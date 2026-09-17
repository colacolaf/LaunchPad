//! The risk/accounting layer, Phase 0 edition: per-user balances per
//! currency in **integers only**, with funds **reserved at place time**.
//!
//! Why reserve rather than check (decision log 2026-09-06): a check-only
//! ledger lets two GTC bids each pass against the same free balance — both
//! can then fill, and the user owes the exchange. That breaks the one
//! accounting invariant an exchange must never break. Reserving makes the
//! invariant structural: `free` can never fund more than it holds, so no
//! sequence of accepts can over-commit a user. This is exchange-core's model
//! — their place command carries a `reservePrice` precisely so that *move*
//! needs no risk re-check.
//!
//! Ground rules (unchanged from the rest of the crate):
//! - **No floats.** Quote costs are computed in integer tick/lot arithmetic
//!   with one ceil-rounding helper (conservative: lock a tick too much, and
//!   the dust is returned on release — rounding may *over*-charge, never
//!   under-charge, so no user can be drained by rounding).
//! - **No overdrafts, by construction.** Amounts are `u64` and every
//!   deduction is checked; a commit that would exceed `free` is rejected,
//!   not clamped.
//! - **Determinism.** No clocks, no randomness — replay-safe (Phase 3 will
//!   prove it via the journal).
//!
//! Phase 3 adds fees ([`FeeSchedule`] + the per-currency sink): charged in
//! the **received asset**, deducted from each side's fill proceeds, floor-
//! rounded so the exchange never over-charges on dust (decision log
//! 2026-09-16). Conservation widens accordingly — deposits now split three
//! ways: Σ(free + locked) + Σ(fees) = deposits.
//!
//! Shipped since this header was written: position limits, maker/taker
//! fees, `reduceOrder`, the command journal (see the decision log). Still
//! deliberately out of scope (per the audit + phases.md): margin modes,
//! per-symbol scales, interest/settlement.

use std::collections::HashMap;

use crate::domain::{CurrencyId, Qty, UserId};

/// Everything the risk layer can reject.
///
/// Hand-rolled like [`crate::domain::DomainError`] / [`crate::book::BookError`]:
/// three variants, eyeball-auditable; dependencies stay at zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskError {
    /// Deposit/withdraw with a zero amount — meaningless, rejected like
    /// zero prices/quantities in the domain.
    ZeroAmount {
        /// The currency the adjustment named.
        currency: CurrencyId,
    },
    /// A withdrawal (or negative adjustment) larger than the free balance.
    InsufficientFree {
        /// The currency of the withdrawal.
        currency: CurrencyId,
        /// Amount requested.
        requested: u64,
        /// Amount actually free.
        available: u64,
    },
    /// A bid move's funding check passed but its new commitment would push
    /// the account's locked funds past its position limit. The move is
    /// rejected before the book sees it; nothing has changed anywhere.
    PositionLimitExceeded {
        /// The currency whose locked funds would breach the cap.
        currency: CurrencyId,
        /// The account's cap on locked funds.
        cap: u64,
        /// What the commit would have locked in total.
        would_lock: u64,
    },
    /// A snapshot's captured account lock disagrees with the sum of its
    /// captured order locks — the snapshot is internally inconsistent (the
    /// two copies of the same fact were edited apart, or corrupted).
    SnapshotLockMismatch {
        /// The account's currency.
        currency: CurrencyId,
        /// The lock the account row captured.
        captured: u64,
        /// The sum the captured order rows imply.
        orders: u64,
    },
    /// Placing an order whose required commitment exceeds the free balance.
    ///
    /// This is the Phase 0 risk gate: the order never reaches the book.
    InsufficientForOrder {
        /// The currency that needed funding.
        currency: CurrencyId,
        /// Amount the order would have to lock.
        required: u64,
        /// Amount actually free.
        available: u64,
    },
}

impl std::fmt::Display for RiskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroAmount { currency } => {
                write!(f, "zero-amount adjustment for currency {currency:?}")
            }
            Self::InsufficientFree {
                currency,
                requested,
                available,
            } => write!(
                f,
                "withdraw of {requested} {currency:?} exceeds free balance {available}"
            ),
            Self::InsufficientForOrder {
                currency,
                required,
                available,
            } => write!(
                f,
                "order needs {required} {currency:?} but only {available} is free"
            ),
            Self::PositionLimitExceeded {
                currency,
                cap,
                would_lock,
            } => write!(
                f,
                "commit would lock {would_lock} {currency:?}, past the position limit {cap}"
            ),
            Self::SnapshotLockMismatch {
                currency,
                captured,
                orders,
            } => write!(
                f,
                "snapshot is inconsistent: account lock {captured} {currency:?} != Σ order locks {orders}"
            ),
        }
    }
}

impl std::error::Error for RiskError {}

/// One (user, currency) cell: free funds plus funds locked by resting orders.
///
/// Both amounts are `u64`, so an overdraft is unrepresentable — the same
/// philosophy as `Price`'s private non-zero inner value. The real invariant:
/// `free` never goes negative (a short commit/withdraw is *rejected*, not
/// clamped), and `locked` changes only through commit (funded from `free`),
/// release (returns to `free`), and settle (draws from the lock) — so every
/// non-deposit/withdraw mutation conserves `free + locked`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Account {
    free: u64,
    locked: u64,
}

/// Quote-currency cost, in ticks, of committing `qty` lots at `price` ticks.
///
/// Units derivation (worth being able to reproduce): `price` ticks means
/// 1.0 base unit costs `price.tick()` ticks, so `q` base units cost
/// `q × price.tick()` ticks; `qty.lot()` lots *are* `q × QTY_SCALE`, so
/// `q = qty.lot() / QTY_SCALE` and
///
/// `cost_ticks = qty_lots × price_ticks / QTY_SCALE`.
///
/// The division is the one place integer accounting could silently create
/// or destroy value, so the rounding direction is a **decision, not an
/// accident**: ceil (conservative — a lock may take one extra tick; the dust
/// is released later with the rest of the lock, so users lose nothing,
/// while under-charging a lock would let a fill cost more than was
/// reserved). Per-symbol scales — which would make this exact rather than
/// ceil — are Phase 3.
///
/// Checked multiplication: a `None` means the commitment itself is too
/// large for `u64` ticks and must be rejected upstream, never wrapped.
///
/// Public because it is the module's pricing vocabulary: the Phase 1 engine
/// calls this to compute a bid's commitment before [`Ledger::commit`].
#[must_use]
pub fn quote_cost_ticks(qty: Qty, price: crate::domain::Price) -> Option<u64> {
    Some(
        qty.lot()
            .checked_mul(price.tick())?
            .checked_add(crate::domain::QTY_SCALE - 1)?
            / crate::domain::QTY_SCALE,
    )
}

/// Quote-currency cost, in ticks, of *settling* `qty` lots at `price` ticks —
/// the FLOOR sibling of [`quote_cost_ticks`].
///
/// Why floor here when the lock uses ceil: the lock is one ceil over the
/// whole order, but settlement happens **per fill**. Ceil is subadditive
/// (`Σ ceil(xᵢ) ≥ ceil(Σ xᵢ)`), so settling each fill with the lock's ceil
/// rule can settle *more* than the single ceil locked — concretely, an order
/// of 3 lots @ 3 ticks locks `ceil(9 / 1e8) = 1` tick, but three 1-lot fills
/// would "owe" `3 × ceil(3/1e8) = 3` ticks and [`Ledger::settle`] would
/// panic at the scene. Floor is safe in the other direction:
///
/// `Σ floor(xᵢ) ≤ floor(Σ xᵢ) ≤ ceil(Σ xᵢ) = lock`,
///
/// so the sum of per-fill floors can never exceed the lock, no matter how
/// the order is split across fills. The un-settled dust stays in the lock
/// and returns to free when the order dies (the engine releases it with the
/// lock remainder); sellers take a sub-tick haircut on dusty fills until
/// per-symbol scales (Phase 3) make settlement exact. (Decision log
/// 2026-09-07: "floor settlement, ceil lock".)
///
/// # Errors
/// `None` when the product overflows `u64`. Impossible for a real fill — a
/// fill's `qty × price` is bounded by the payer's own place-time lock math,
/// which was already checked with the ceil helper — but the `Option` keeps
/// the overflow invariant explicit at every call site instead of trusting
/// the argument.
#[must_use]
pub fn quote_cost_floor_ticks(qty: Qty, price: crate::domain::Price) -> Option<u64> {
    Some(qty.lot().checked_mul(price.tick())? / crate::domain::QTY_SCALE)
}

/// Maker/taker fee rates in **integer basis points** (1 bp = 1/10_000).
///
/// Constructed with validation: a rate above 10_000 bps would exceed 100% —
/// structurally impossible to pay out of the received value — so it is a
/// constructor rejection, not a runtime hazard. A 0/0 schedule is the exact
/// no-fees behavior (every fee computation floors to zero; the sink never
/// moves), which is how the benches and every pre-fee test stay byte-
/// identical.
///
/// Why the rates live on the schedule rather than per call: the fee model is
/// *engine configuration* (decision row 52) — one decision, validated once,
/// and (Phase 3, journal slice) recorded in the journal header so replay
/// reconstructs the same schedule. Copy-semantic and cheap: the engine holds
/// it by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeSchedule {
    /// Fee charged on what the resting (liquidity-*providing*) side
    /// receives, in basis points of the received value.
    pub maker_bps: u64,
    /// Fee charged on what the incoming (liquidity-*taking*) side
    /// receives, in basis points of the received value.
    pub taker_bps: u64,
}

impl FeeSchedule {
    /// The no-fees schedule — benches and pre-fee behavior, exactly.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            maker_bps: 0,
            taker_bps: 0,
        }
    }

    /// Validate and construct. Rates are independent; each must be ≤ 10_000
    /// (100%). 10_000 itself is legal — a 100% fee zeroes the receipt, odd
    /// but coherent.
    ///
    /// # Errors
    /// [`FeeError::RateTooHigh`] — either rate exceeds 10_000 bps.
    pub fn new(maker_bps: u64, taker_bps: u64) -> Result<Self, FeeError> {
        if maker_bps > BPS_DENOMINATOR || taker_bps > BPS_DENOMINATOR {
            return Err(FeeError::RateTooHigh {
                maker_bps,
                taker_bps,
            });
        }
        Ok(Self {
            maker_bps,
            taker_bps,
        })
    }

    /// Floor fee on `received`, at this schedule's `rate` field.
    ///
    /// Overflow-proof by construction: a naive `received × bps` overflows
    /// `u64` for a received value above ~1.8e15 (a legal fill — the buyer's
    /// own base receipt can be that large), so the product is split into
    /// `(received / DENOM) × bps + (received % DENOM) × bps / DENOM` — the
    /// first term needs `bps × bps ≤ 1e8` (holds: bps ≤ 10_000), the second
    /// stays below the denominator. Floor semantics: every term floors,
    /// so the total fee can never exceed the received value (the maximum
    /// 10_000-bps schedule collects exactly `received`, never more).
    #[must_use]
    pub fn fee_of(&self, received: u64, rate: u64) -> u64 {
        debug_assert!(rate <= BPS_DENOMINATOR, "schedule rates are validated");
        (received / BPS_DENOMINATOR)
            .wrapping_mul(rate)
            .wrapping_add((received % BPS_DENOMINATOR).wrapping_mul(rate) / BPS_DENOMINATOR)
    }

    /// Maker-side fee on a received value.
    #[must_use]
    pub fn maker_fee(&self, received: u64) -> u64 {
        self.fee_of(received, self.maker_bps)
    }

    /// Taker-side fee on a received value.
    #[must_use]
    pub fn taker_fee(&self, received: u64) -> u64 {
        self.fee_of(received, self.taker_bps)
    }
}

/// Everything the fee configuration can reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeError {
    /// A rate above 10_000 bps (100%) can never be paid out of the value
    /// it taxes — rejected at construction, not discovered mid-settlement.
    RateTooHigh {
        /// The maker rate that was offered.
        maker_bps: u64,
        /// The taker rate that was offered.
        taker_bps: u64,
    },
}

impl std::fmt::Display for FeeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateTooHigh {
                maker_bps,
                taker_bps,
            } => write!(
                f,
                "fee rates {maker_bps}/{taker_bps} bps exceed the 10_000 bps (100%) maximum"
            ),
        }
    }
}

impl std::error::Error for FeeError {}

/// Basis-point denominator: 10_000 bps = 100%. Module-private — callers
/// speak in bps and fees, never in the raw denominator.
const BPS_DENOMINATOR: u64 = 10_000;

/// The (user, currency) → account ledger.
///
/// One `Ledger` per exchange (all symbols share the currency space, matching
/// exchange-core's per-user multi-currency accounts). The engine facade that
/// ties book + ledger into one atomic operation model is Phase 1; for now
/// tests script both sides explicitly.
#[derive(Debug, Default)]
pub struct Ledger {
    accounts: HashMap<(UserId, CurrencyId), Account>,
    /// Fees collected per currency, exchange-owned. Grows only via
    /// [`Ledger::collect_fee`]; conservation counts it as the third term:
    /// Σ(free + locked) + Σ(fees) = deposits.
    fees: HashMap<CurrencyId, u64>,
    /// Per-(user, currency) caps on **locked** funds — the Phase 3 position
    /// limit (decision row 54). Absent = uncapped; the map only ever holds
    /// accounts someone explicitly capped, so uncapped traffic pays zero
    /// lookups. Set/cleared via [`Ledger::set_position_limit`] /
    /// [`Ledger::clear_position_limit`].
    position_limits: HashMap<(UserId, CurrencyId), u64>,
}

impl Ledger {
    /// An empty ledger — no users, no balances, no fees collected.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fees collected so far, in `currency` — the exchange's take. Unknown
    /// currencies read as zero (same rule as [`Ledger::free`]).
    #[must_use]
    pub fn fees_collected(&self, currency: CurrencyId) -> u64 {
        self.fees.get(&currency).copied().unwrap_or(0)
    }

    /// Cap the (user, currency) account's **locked** funds at `cap` — the
    /// Phase 3 position limit. The cap binds open-order exposure, not
    /// wealth: free funds are untouched by it. `cap = 0` is legal and
    /// blocks every commit for the account. A cap set below the currently
    /// locked amount takes effect immediately (the next commit that would
    /// grow the lock is rejected; existing locks run off normally).
    ///
    /// `cap = 0` is the freeze control: no commit can pass for the
    /// account until the cap is cleared or raised. (Unlike deposits and
    /// orders, a cap of zero is meaningful — it is a rule about future
    /// commitments, not an amount to move — so the zero-amount rejection
    /// does not apply here.)
    pub fn set_position_limit(&mut self, user: UserId, currency: CurrencyId, cap: u64) {
        self.position_limits.insert((user, currency), cap);
    }

    /// Remove a (user, currency) position limit — the account returns to
    /// uncapped. Clearing an absent limit is a no-op (idempotent).
    pub fn clear_position_limit(&mut self, user: UserId, currency: CurrencyId) {
        self.position_limits.remove(&(user, currency));
    }

    /// The account's cap on locked funds, if one is set.
    #[must_use]
    pub fn position_limit(&self, user: UserId, currency: CurrencyId) -> Option<u64> {
        self.position_limits.get(&(user, currency)).copied()
    }

    /// Collect `amount` of `currency` as fees from a participant's *free*
    /// balance — the fee leg of a fill, deducted from what that side just
    /// received (decision row 52: fees live in the received asset).
    ///
    /// Panics on a short free balance: by construction the caller collects
    /// *after* [`Ledger::settle`] credited this side's proceeds and the fee
    /// is floored to ≤ those proceeds — a miss is an engine bug (fail loudly
    /// at the scene, same philosophy as [`Ledger::release`]).
    pub fn collect_fee(&mut self, payer: UserId, currency: CurrencyId, amount: u64) {
        if amount == 0 {
            return; // 0-fee schedules and dusty fills contribute nothing.
        }
        let account = self
            .accounts
            .get_mut(&(payer, currency))
            .expect("fee collection follows the settle that credited the proceeds");
        account.free = account
            .free
            .checked_sub(amount)
            .expect("fee ≤ the just-credited proceeds, floored — never short");
        let sink = self.fees.entry(currency).or_default();
        *sink = sink
            .checked_add(amount)
            .expect("fee sink overflow is a multi-u64-worth-of-trades bug, not a value");
    }

    /// Every funded account as `(user, currency, free, locked)`, sorted —
    /// the journal's replay digest folds all of it, because aggregates
    /// cannot see per-account drift (a free/locked swap between two users
    /// conserves the total but is a different state).
    #[must_use]
    pub fn accounts(&self) -> Vec<(UserId, CurrencyId, u64, u64)> {
        let mut rows: Vec<_> = self
            .accounts
            .iter()
            .map(|(&(user, currency), account)| (user, currency, account.free, account.locked))
            .collect();
        rows.sort_unstable();
        rows
    }

    /// The fee sink: `(currency, collected)` per currency that ever
    /// collected a fee, sorted — the replay digest's third money term.
    #[must_use]
    pub fn fee_sink(&self) -> Vec<(CurrencyId, u64)> {
        let mut rows: Vec<_> = self
            .fees
            .iter()
            .map(|(&currency, &collected)| (currency, collected))
            .collect();
        rows.sort_unstable();
        rows
    }

    /// Every position limit as `(user, currency, cap)`, sorted — part of
    /// the engine's observable configuration, so replay must reproduce it.
    #[must_use]
    pub fn position_limits(&self) -> Vec<(UserId, CurrencyId, u64)> {
        let mut rows: Vec<_> = self
            .position_limits
            .iter()
            .map(|(&(user, currency), &cap)| (user, currency, cap))
            .collect();
        rows.sort_unstable();
        rows
    }

    /// Free balance for one (user, currency). Unknown pairs read as zero —
    /// an account that never traded has no entry, and materializing one on
    /// read would be pure waste.
    #[must_use]
    pub fn free(&self, user: UserId, currency: CurrencyId) -> u64 {
        self.accounts
            .get(&(user, currency))
            .map_or(0, |account| account.free)
    }

    /// Locked balance for one (user, currency) — funds committed to resting
    /// orders, not spendable until released.
    #[must_use]
    pub fn locked(&self, user: UserId, currency: CurrencyId) -> u64 {
        self.accounts
            .get(&(user, currency))
            .map_or(0, |account| account.locked)
    }

    /// Credit an account (deposit). Creates the pair on first touch.
    ///
    /// # Errors
    /// [`RiskError::ZeroAmount`] — zero adjustments are rejected like zero
    /// orders, not silently ignored (a silent no-op could mask a bug in the
    /// caller's command stream).
    pub fn deposit(
        &mut self,
        user: UserId,
        currency: CurrencyId,
        amount: u64,
    ) -> Result<(), RiskError> {
        self.adjust(user, currency, amount, 0)
    }

    /// Debit an account (withdrawal) from *free* funds only — locked funds
    /// belong to resting orders until they release.
    ///
    /// # Errors
    /// - [`RiskError::ZeroAmount`] — zero adjustments rejected;
    /// - [`RiskError::InsufficientFree`] — more than free requested.
    pub fn withdraw(
        &mut self,
        user: UserId,
        currency: CurrencyId,
        amount: u64,
    ) -> Result<(), RiskError> {
        self.adjust(user, currency, 0, amount)
    }

    /// Shared deposit/withdraw path: `+amount` and `-amount` in one place so
    /// the zero-check and the checked math can't drift apart.
    fn adjust(
        &mut self,
        user: UserId,
        currency: CurrencyId,
        add: u64,
        sub: u64,
    ) -> Result<(), RiskError> {
        if add == 0 && sub == 0 {
            return Err(RiskError::ZeroAmount { currency });
        }
        let account = self.accounts.entry((user, currency)).or_default();
        if sub > 0 {
            account.free = account
                .free
                .checked_sub(sub)
                .ok_or(RiskError::InsufficientFree {
                    currency,
                    requested: sub,
                    available: account.free,
                })?;
        }
        // Checked for symmetry: a deposit near u64::MAX fails loudly instead
        // of wrapping into a tiny balance.
        account.free = account
            .free
            .checked_add(add)
            .ok_or(RiskError::InsufficientFree {
                currency,
                requested: add,
                available: account.free,
            })?;
        Ok(())
    }

    /// Commit funds for a new order (the place-time lock).
    ///
    /// For a **bid**: `required` quote ticks are moved free → locked.
    /// For an **ask**: `required` base lots are moved free → locked.
    /// The caller computes `required` (bid: [`quote_cost_ticks`] on the
    /// order's price — market bids must use their worst acceptable price;
    /// ask: the order quantity in lots). All-or-nothing: a short balance
    /// leaves the account untouched and the order is rejected.
    ///
    /// # Errors
    /// [`RiskError::InsufficientForOrder`] — free < required. The account is
    /// unchanged (checked subtraction happens *before* the add).
    pub fn commit(
        &mut self,
        user: UserId,
        currency: CurrencyId,
        required: u64,
    ) -> Result<(), RiskError> {
        // Position-limit gate (decision row 54) — BEFORE any mutation, so a
        // rejected commit leaves the account untouched (same contract as
        // the funds check below).
        if let Some(&cap) = self.position_limits.get(&(user, currency)) {
            let already_locked = self
                .accounts
                .get(&(user, currency))
                .map_or(0, |account| account.locked);
            let would_lock = already_locked
                .checked_add(required)
                .expect("locked + required cannot overflow: both came out of free funds");
            if would_lock > cap {
                return Err(RiskError::PositionLimitExceeded {
                    currency,
                    cap,
                    would_lock,
                });
            }
        }
        let account = self.accounts.entry((user, currency)).or_default();
        // Checked subtraction first: on failure the entry may exist but is
        // unmodified (free unchanged, locked unchanged).
        let new_free =
            account
                .free
                .checked_sub(required)
                .ok_or(RiskError::InsufficientForOrder {
                    currency,
                    required,
                    available: account.free,
                })?;
        account.free = new_free;
        account.locked = account
            .locked
            .checked_add(required)
            .expect("locked ≤ free + locked always; free just absorbed the subtraction");
        Ok(())
    }

    /// Release a previous [`Ledger::commit`] — order cancelled, or partially
    /// filled and the unused remainder of the lock goes back to free.
    ///
    /// # Errors / panics
    /// Panics if `amount` exceeds what this pair has locked: that is an
    /// engine bug (double-release or mismatched lock), not a user input
    /// problem, and the "index and book are in sync" rule applies — fail
    /// loudly at the scene rather than paper over it.
    pub fn release(&mut self, user: UserId, currency: CurrencyId, amount: u64) {
        let account = self
            .accounts
            .get_mut(&(user, currency))
            .expect("release only ever follows a commit on the same pair");
        account.locked = account
            .locked
            .checked_sub(amount)
            .expect("release amount never exceeds the lock");
        account.free = account
            .free
            .checked_add(amount)
            .expect("free + released cannot overflow: released was previously part of free");
    }

    /// Create/overwrite an account's balances from a snapshot row (the
    /// snapshot-restore path, decision row 61). Not part of the trading API:
    /// restore sets captured state **exactly** — free and locked alike —
    /// instead of re-deriving it, because a bid's lock is dust-bearing
    /// (`ceil(total×p) − Σfloor(fill_i×p)`) and no sequence of deposit/commit
    /// calls reproduces it. Validation (lock = Σ order locks, lock ≤ free,
    /// cap respected) is the ENGINE's restore-time job, checked before this
    /// is ever called; conservation holds because both columns come from a
    /// state that satisfied it.
    pub fn restore_account(&mut self, user: UserId, currency: CurrencyId, free: u64, locked: u64) {
        self.accounts
            .insert((user, currency), Account { free, locked });
    }

    /// Set the fee sink for one currency to a captured value (the
    /// snapshot-restore path). Not part of the trading API — the sink only
    /// ever grows via [`Ledger::collect_fee`] during trading; restore
    /// reproduces the captured total exactly.
    pub fn restore_fee_sink(&mut self, currency: CurrencyId, collected: u64) {
        self.fees.insert(currency, collected);
    }

    /// Settle a fill: move `amount` of `currency` from `payer` to `payee`.
    ///
    /// The payer's share comes out of its **lock** (it was committed at
    /// place); the payee receives it as **free**. Buyer pays quote, seller
    /// receives quote; seller delivers base, buyer receives base — same
    /// method both directions, which is what makes conservation checkable:
    /// the amount is charged and credited once, computed by the *caller*
    /// with the same helper, so both sides always see the same number.
    ///
    /// # Errors / panics
    /// Panics if the payer's lock is short: an engine that settles more than
    /// it locked is broken by definition (see [`Ledger::release`]).
    pub fn settle(&mut self, payer: UserId, payee: UserId, currency: CurrencyId, amount: u64) {
        let payer_account = self
            .accounts
            .get_mut(&(payer, currency))
            .expect("settle only ever draws from a committed lock");
        payer_account.locked = payer_account
            .locked
            .checked_sub(amount)
            .expect("settled amount never exceeds the lock");
        if payer != payee {
            // Self-trade (payer == payee) needs no transfer — the lock simply
            // evaporates into the same account's free funds. Phase 3 adds
            // self-match prevention; until then this stays correct.
            let payee_account = self.accounts.entry((payee, currency)).or_default();
            payee_account.free = payee_account
                .free
                .checked_add(amount)
                .expect("payee credit cannot overflow a sane command stream");
        } else {
            // Same account: locked amount returns to free.
            let account = self
                .accounts
                .get_mut(&(payer, currency))
                .expect("just read");
            account.free = account
                .free
                .checked_add(amount)
                .expect("same-account release cannot overflow");
        }
    }
}

#[cfg(test)]
mod tests;
