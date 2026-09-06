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
//! Deliberately out of scope (Phase 3 per `docs/TODO.md` §5): position
//! limits, margin modes, fees, per-symbol scales, interest/settlement.

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
        }
    }
}

impl std::error::Error for RiskError {}

/// One (user, currency) cell: free funds plus funds locked by resting orders.
///
/// Both amounts are `u64`, so an overdraft is unrepresentable — the same
/// philosophy as `Price`'s private non-zero inner value. Invariant:
/// `locked ≤ free + locked` trivially, and every mutation keeps
/// `free ≥ 0` by checked arithmetic or rejection.
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

/// The (user, currency) → account ledger.
///
/// One `Ledger` per exchange (all symbols share the currency space, matching
/// exchange-core's per-user multi-currency accounts). The engine facade that
/// ties book + ledger into one atomic operation model is Phase 1; for now
/// tests script both sides explicitly.
#[derive(Debug, Default)]
pub struct Ledger {
    accounts: HashMap<(UserId, CurrencyId), Account>,
}

impl Ledger {
    /// An empty ledger — no users, no balances.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
mod tests {
    use super::*;
    use crate::domain::Price;

    const USD: CurrencyId = CurrencyId(1);
    const BTC: CurrencyId = CurrencyId(2);
    const ALICE: UserId = UserId(1);
    const BOB: UserId = UserId(2);

    fn ledger_with(alice_usd: u64, alice_btc: u64, bob_usd: u64, bob_btc: u64) -> Ledger {
        let mut ledger = Ledger::new();
        for (user, currency, amount) in [
            (ALICE, USD, alice_usd),
            (ALICE, BTC, alice_btc),
            (BOB, USD, bob_usd),
            (BOB, BTC, bob_btc),
        ] {
            if amount > 0 {
                ledger.deposit(user, currency, amount).unwrap();
            }
        }
        ledger
    }

    // ---- read path -------------------------------------------------------

    #[test]
    fn unknown_pairs_read_as_zero() {
        let ledger = Ledger::new();
        assert_eq!(ledger.free(ALICE, USD), 0);
        assert_eq!(ledger.locked(ALICE, USD), 0);
    }

    #[test]
    fn deposit_creates_and_accumulates() {
        let mut ledger = Ledger::new();
        ledger.deposit(ALICE, USD, 1_000).unwrap();
        ledger.deposit(ALICE, USD, 500).unwrap();
        assert_eq!(ledger.free(ALICE, USD), 1_500);
        assert_eq!(ledger.locked(ALICE, USD), 0);
    }

    // ---- deposit/withdraw rejections -------------------------------------

    #[test]
    fn zero_adjustments_are_rejected_not_ignored() {
        let mut ledger = Ledger::new();
        assert_eq!(
            ledger.deposit(ALICE, USD, 0),
            Err(RiskError::ZeroAmount { currency: USD })
        );
        assert_eq!(
            ledger.withdraw(ALICE, USD, 0),
            Err(RiskError::ZeroAmount { currency: USD })
        );
        // And nothing was created by the failed attempts:
        assert_eq!(ledger.free(ALICE, USD), 0);
    }

    #[test]
    fn withdraw_more_than_free_is_rejected_untouched() {
        let mut ledger = ledger_with(1_000, 0, 0, 0);
        assert_eq!(
            ledger.withdraw(ALICE, USD, 1_001),
            Err(RiskError::InsufficientFree {
                currency: USD,
                requested: 1_001,
                available: 1_000,
            })
        );
        assert_eq!(
            ledger.free(ALICE, USD),
            1_000,
            "account unchanged on reject"
        );
    }

    #[test]
    fn withdraw_cannot_touch_locked_funds() {
        let mut ledger = ledger_with(1_000, 0, 0, 0);
        ledger.commit(ALICE, USD, 600).unwrap();
        // 400 free, 600 locked. A withdraw of 500 must fail even though the
        // account "holds" 1_000 — locked funds belong to resting orders.
        assert!(matches!(
            ledger.withdraw(ALICE, USD, 500),
            Err(RiskError::InsufficientFree { .. })
        ));
        assert_eq!(ledger.free(ALICE, USD), 400);
        assert_eq!(ledger.locked(ALICE, USD), 600);
    }

    // ---- commit (the place-time lock) -------------------------------------

    #[test]
    fn commit_moves_free_to_locked() {
        let mut ledger = ledger_with(1_000, 0, 0, 0);
        ledger.commit(ALICE, USD, 400).unwrap();
        assert_eq!(ledger.free(ALICE, USD), 600);
        assert_eq!(ledger.locked(ALICE, USD), 400);
    }

    #[test]
    fn commit_beyond_free_is_rejected_all_or_nothing() {
        let mut ledger = ledger_with(1_000, 0, 0, 0);
        assert_eq!(
            ledger.commit(ALICE, USD, 1_001),
            Err(RiskError::InsufficientForOrder {
                currency: USD,
                required: 1_001,
                available: 1_000,
            })
        );
        // All-or-nothing: not even a partial lock happened.
        assert_eq!(ledger.free(ALICE, USD), 1_000);
        assert_eq!(ledger.locked(ALICE, USD), 0);
    }

    #[test]
    fn second_bid_cannot_double_spend_the_same_free_balance() {
        // THE over-commitment case that decided reserve-vs-check: deposit
        // 100 once, place two bids that each need 100. The second must
        // reject — a check-only ledger would have accepted both.
        let mut ledger = ledger_with(100, 0, 0, 0);
        ledger.commit(ALICE, USD, 100).unwrap();
        assert!(matches!(
            ledger.commit(ALICE, USD, 100),
            Err(RiskError::InsufficientForOrder { .. })
        ));
        assert_eq!(ledger.free(ALICE, USD), 0);
        assert_eq!(ledger.locked(ALICE, USD), 100);
    }

    #[test]
    fn ask_commits_base_lots_bid_commits_quote_ticks() {
        // Same ledger API, different currency per side — the *engine* decides
        // which currency an order draws on; the ledger just moves integers.
        let mut ledger = ledger_with(5_000_000, 3, 0, 0);
        ledger.commit(ALICE, BTC, 2).unwrap(); // ask: lock 2 base lots
        assert_eq!(ledger.free(ALICE, BTC), 1);
        ledger.commit(ALICE, USD, 5_000_000).unwrap(); // bid: lock quote
        assert_eq!(ledger.free(ALICE, USD), 0);
    }

    // ---- release (cancel / unused remainder) ------------------------------

    #[test]
    fn release_returns_exactly_the_locked_amount() {
        let mut ledger = ledger_with(1_000, 0, 0, 0);
        ledger.commit(ALICE, USD, 400).unwrap();
        ledger.release(ALICE, USD, 400);
        assert_eq!(ledger.free(ALICE, USD), 1_000);
        assert_eq!(ledger.locked(ALICE, USD), 0);
    }

    #[test]
    fn partial_fill_releases_the_unused_remainder_of_the_lock() {
        // Locked 400; a fill consumes 250 of it; the other 150 returns.
        let mut ledger = ledger_with(1_000, 0, 0, 0);
        ledger.commit(ALICE, USD, 400).unwrap();
        ledger.settle(ALICE, BOB, USD, 250);
        assert_eq!(ledger.locked(ALICE, USD), 150);
        ledger.release(ALICE, USD, 150);
        assert_eq!(ledger.free(ALICE, USD), 750);
        assert_eq!(ledger.locked(ALICE, USD), 0);
    }

    // ---- settle (fills move value between users) ---------------------------

    #[test]
    fn settle_moves_value_payer_to_payee_only() {
        let mut ledger = ledger_with(1_000, 0, 0, 0);
        ledger.commit(ALICE, USD, 400).unwrap();
        ledger.settle(ALICE, BOB, USD, 250);
        // Payer's lock shrinks by exactly the fill; payee gains free funds.
        assert_eq!(ledger.locked(ALICE, USD), 150);
        assert_eq!(ledger.free(BOB, USD), 250);
        // Conservation inside the ledger: 750 + 150 + 250 == 1_000.
        assert_eq!(
            ledger.free(ALICE, USD) + ledger.locked(ALICE, USD) + ledger.free(BOB, USD),
            1_000
        );
    }

    #[test]
    fn self_trade_returns_lock_to_free_without_transfer() {
        let mut ledger = ledger_with(1_000, 0, 0, 0);
        ledger.commit(ALICE, USD, 400).unwrap();
        ledger.settle(ALICE, ALICE, USD, 250);
        assert_eq!(ledger.locked(ALICE, USD), 150);
        assert_eq!(ledger.free(ALICE, USD), 850);
        // Total unchanged: 850 + 150 == 1_000.
    }

    // ---- the ceil-rounding helper ------------------------------------------

    #[test]
    fn quote_cost_is_exact_when_divisible() {
        // 1.0 base = QTY_SCALE lots at 150.0 quote (1_500_000 ticks):
        // cost = 100_000_000 × 1_500_000 / 100_000_000 = 1_500_000 ticks exact.
        let qty = Qty::from_lots(100_000_000).unwrap(); // exactly 1.0 base
        let price = Price::from_ticks(1_500_000).unwrap(); // exactly 150.0 quote
        assert_eq!(quote_cost_ticks(qty, price), Some(1_500_000));
    }

    #[test]
    fn quote_cost_ceils_never_floors() {
        // 1 lot (1e-8 base) at 1 tick: exact cost is 1e-8 ticks — rounds UP
        // to 1 tick. Under-charging (0) would let a lock cover a fill it
        // cannot pay for. (First draft of this test expected "30_000" here —
        // the tutor conflated lots with base units; the property test below
        // is what caught it. Lots are 1e-8 base: order sizes in lots are
        // huge numbers, costs in ticks stay readable.)
        let qty = Qty::from_lots(1).unwrap();
        let price = Price::from_ticks(1).unwrap();
        assert_eq!(quote_cost_ticks(qty, price), Some(1));
    }

    #[test]
    fn quote_cost_overflow_is_none_not_wrap() {
        let qty = Qty::from_lots(u64::MAX).unwrap();
        let price = Price::from_ticks(u64::MAX).unwrap();
        assert_eq!(quote_cost_ticks(qty, price), None);
    }

    #[test]
    fn quote_cost_realistic_order_with_remainder() {
        // 1.23456789 base at 98765.4321 quote:
        // 123_456_789 lots × 987_654_321 ticks = 121_932_631_112_635_269,
        // / 1e8 = 1_219_326_311.1263… → ceil → 1_219_326_312 ticks.
        let qty = Qty::from_lots(123_456_789).unwrap();
        let price = Price::from_ticks(987_654_321).unwrap();
        assert_eq!(quote_cost_ticks(qty, price), Some(1_219_326_312));
    }

    // ---- error display smoke ------------------------------------------------

    #[test]
    fn errors_render_readably() {
        let err = RiskError::InsufficientForOrder {
            currency: USD,
            required: 10,
            available: 4,
        };
        let msg = err.to_string();
        assert!(msg.contains("10") && msg.contains("4"));
        assert!(matches!(
            RiskError::ZeroAmount { currency: BTC },
            RiskError::ZeroAmount { .. }
        ));
    }

    // ---- Property tests ----------------------------------------------------

    use proptest::prelude::*;

    /// One randomized ledger operation. Deposit/withdraw exercise the
    /// external funding commands; Commit/Release/Settle exercise the
    /// order lifecycle the Phase 1 engine will drive.
    #[derive(Debug, Clone, Copy)]
    enum LCmd {
        Deposit { pick: u64, amount: u64 },
        Withdraw { pick: u64, amount: u64 },
        Commit { pick: u64, amount: u64 },
        Release { pick: u64, amount: u64 },
        Settle { from: u64, to: u64, amount: u64 },
    }

    fn lcmd_strategy() -> impl Strategy<Value = LCmd> {
        // Small user space: collisions between users are frequent, which is
        // where transfer bugs live. Amounts kept small: rejections stay
        // common, which is itself a path we want exercised.
        let user = 0u64..4;
        let amount = 1u64..50;
        prop_oneof![
            2 => (user.clone(), amount.clone()).prop_map(|(pick, amount)| LCmd::Deposit { pick, amount }),
            1 => (user.clone(), amount.clone()).prop_map(|(pick, amount)| LCmd::Withdraw { pick, amount }),
            3 => (user.clone(), amount.clone()).prop_map(|(pick, amount)| LCmd::Commit { pick, amount }),
            2 => (user.clone(), amount.clone()).prop_map(|(pick, amount)| LCmd::Release { pick, amount }),
            2 => (user.clone(), user, amount).prop_map(|(from, to, amount)| LCmd::Settle { from, to, amount }),
        ]
    }

    /// Drive a randomized command sequence through the ledger with a
    /// model that mirrors the intended semantics, checking after every op:
    ///
    /// 1. **Conservation** — total (free + locked) across users changes only
    ///    by deposit (+) and withdraw (−). Every commit/release/settle is
    ///    internal reshuffling.
    /// 2. **No overdraft** — free ≥ 0 and locked ≥ 0 for every account,
    ///    always (u64 makes negatives unrepresentable; the property asserts
    ///    the *model* and the ledger agree, which catches semantic drift).
    ///
    /// The model tracks free/locked per user with plain arithmetic and
    /// skips ops the real ledger would reject — except conservation
    /// violations, which must never happen and would fail loudly here.
    fn run_ledger(cmds: &[LCmd]) {
        use crate::domain::{CurrencyId, UserId};
        let cur = CurrencyId(7);
        let users: Vec<UserId> = (1..=4).map(UserId).collect();
        let uid = |pick: u64| users[(pick % 4) as usize];

        let mut ledger = Ledger::new();
        // Model: (free, locked) per user. Deposits are the only external in,
        // withdrawals the only external out.
        let mut model: Vec<(u64, u64)> = vec![(0, 0); 4];
        let mut total_deposited: u64 = 0;

        for cmd in cmds {
            match *cmd {
                LCmd::Deposit { pick, amount } => {
                    ledger.deposit(uid(pick), cur, amount).unwrap();
                    model[(pick % 4) as usize].0 += amount;
                    total_deposited += amount;
                }
                LCmd::Withdraw { pick, amount } => {
                    let idx = (pick % 4) as usize;
                    if model[idx].0 >= amount {
                        ledger.withdraw(uid(pick), cur, amount).unwrap();
                        model[idx].0 -= amount;
                        total_deposited -= amount;
                    } else {
                        assert!(matches!(
                            ledger.withdraw(uid(pick), cur, amount),
                            Err(RiskError::InsufficientFree { .. })
                        ));
                    }
                }
                LCmd::Commit { pick, amount } => {
                    let idx = (pick % 4) as usize;
                    if model[idx].0 >= amount {
                        ledger.commit(uid(pick), cur, amount).unwrap();
                        model[idx].0 -= amount;
                        model[idx].1 += amount;
                    } else {
                        assert!(matches!(
                            ledger.commit(uid(pick), cur, amount),
                            Err(RiskError::InsufficientForOrder { .. })
                        ));
                    }
                }
                LCmd::Release { pick, amount } => {
                    let idx = (pick % 4) as usize;
                    // Only release what the model says is locked — a release
                    // beyond the lock is an engine bug the ledger may panic
                    // on, and the model exists to prevent driving it there.
                    let capped = amount.min(model[idx].1);
                    if capped > 0 {
                        ledger.release(uid(pick), cur, capped);
                        model[idx].1 -= capped;
                        model[idx].0 += capped;
                    }
                }
                LCmd::Settle { from, to, amount } => {
                    let fi = (from % 4) as usize;
                    // Settle draws from the payer's lock only.
                    let capped = amount.min(model[fi].1);
                    if capped > 0 {
                        let ti = (to % 4) as usize;
                        ledger.settle(uid(from), uid(to), cur, capped);
                        model[fi].1 -= capped;
                        if fi != ti {
                            model[ti].0 += capped;
                        } else {
                            model[fi].0 += capped;
                        }
                    }
                }
            }

            // Conservation, after every operation: model total == deposited.
            let model_total: u64 = model.iter().map(|(f, l)| f + l).sum();
            assert_eq!(
                model_total, total_deposited,
                "model diverged from conservation after {cmd:?}"
            );
            // Ledger agrees with the model, account by account.
            for (idx, user) in users.iter().enumerate() {
                assert_eq!(
                    (ledger.free(*user, cur), ledger.locked(*user, cur)),
                    model[idx],
                    "ledger/model divergence for user {user:?} after {cmd:?}"
                );
            }
        }
    }

    proptest! {
        /// Conservation + model agreement under randomized op sequences.
        #[test]
        fn ledger_conserves_total_under_random_ops(
            cmds in prop::collection::vec(lcmd_strategy(), 0..=80),
        ) {
            run_ledger(&cmds);
        }
    }
}
