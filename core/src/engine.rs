//! The engine facade: atomic `place → lock → settle` over the book + ledger.
//!
//! Phase 0 proved the pieces; the integration test (`core/tests/
//! order_lifecycle.rs`) had to *play* this component by hand — computing
//! commitments, settling each fill, releasing locks. This module is the real
//! one: a single owner of the order lifecycle and the money behind it, so a
//! partial failure can never leave a user's balance charged for an order the
//! book rejected (or an order resting that was never funded).
//!
//! The three decisions that shape it (decision log 2026-09-07):
//!
//! 1. **Lock with CEIL, settle per fill with FLOOR.** The lock is one ceil
//!    over the whole order ([`quote_cost_ticks`]), but settlement happens
//!    per fill — and ceil is *subadditive* (`Σ ceil(xᵢ) ≥ ceil(Σ xᵢ)`), so
//!    per-fill ceil can settle more than the lock (3 lots @ 3 ticks locks
//!    1 tick; three 1-lot fills would "owe" 3). Floor is safe in the other
//!    direction (`Σ floor ≤ ceil(Σ) = lock`), and the un-settled dust stays
//!    in the lock until the order dies, then returns to free. Sellers take
//!    a sub-tick haircut on dusty fills; per-symbol scales (Phase 3) make
//!    settlement exact. See [`quote_cost_floor_ticks`].
//! 2. **A market bid is a Limit-IOC at its reserve price.** A market order
//!    has no price bound, so there is nothing to lock against — [`Engine::place`]
//!    requires `reserve: Some(price)` for market bids and rewrites the order
//!    to `Limit { price: reserve } + Ioc`: sweep up to the reserve, kill the
//!    remainder, never rest. This both funds the bid and bounds every fill's
//!    settlement (maker prices are ≤ the reserve). Market asks need no
//!    reserve: the seller's lock is base lots, and quote received is never
//!    owed.
//! 3. **Every operation is a saga** — each step's inverse is guaranteed to
//!    succeed:
//!    - *place*: commit → book → settle fills; on book rejection, release
//!      exactly what was committed (compensation) so the failure is
//!      invisible to the ledger;
//!    - *move* (bids): funds check `free + this order's lock ≥ new cost` →
//!      book move (a [`BookError::WouldCross`] rejects *before* any money
//!      moves) → release old lock → commit new (cannot fail given the
//!      check); asks touch no money (their lock is price-independent lots);
//!    - *cancel*: book removes → release the whole remaining lock, dust
//!      included.
//!
//! Ground rules unchanged from the rest of the crate: no floats, no clocks,
//! no `unsafe`, and panics only on internal-invariant violations (same
//! philosophy as [`Ledger::settle`] — fail loudly at the scene of an engine
//! bug, never reject user input with a panic).

use std::collections::HashMap;
use std::fmt;

use crate::book::{BookError, Fill, OrderBook, PlaceOutcome, ReduceOutcome};
use crate::domain::{
    CurrencyId, Order, OrderId, OrderType, Price, Qty, Side, SymbolId, TimeInForce, UserId,
};
use crate::risk::{
    FeeError, FeeSchedule, Ledger, RiskError, quote_cost_floor_ticks, quote_cost_ticks,
};

/// What [`Engine::place`] did with an incoming order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineOutcome {
    /// The order executed (fully or partially). [`PlaceOutcome`]'s meaning,
    /// restated at the engine level: `fills` in execution order, `resting`
    /// only for a GTC limit with an unfilled remainder.
    Executed {
        /// Trades executed against the opposite side.
        fills: Vec<Fill>,
        /// Quantity now resting on the book, if any.
        resting: Option<Qty>,
    },
    /// A FOK order was killed — all-or-nothing could not be met, so nothing
    /// traded and the whole lock was released. Distinguishable from a
    /// zero-fill IOC exactly because the caller needs to know the difference:
    /// an IOC *chose* not to trade; a FOK *refused* to.
    KilledFok,
}

/// Everything the engine layer can reject, beyond what book and risk
/// already reject (surfaced here as wrapped variants).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The book rejected the operation (unknown/duplicate order, wrong
    /// symbol, a move that would lock or cross).
    Book(BookError),
    /// The risk layer rejected the operation (insufficient free funds for
    /// the order's commitment).
    Risk(RiskError),
    /// A market bid was placed without a reserve price — there is nothing
    /// to lock against, so the engine refuses it rather than inventing a
    /// bound the caller never authorized.
    MarketBidRequiresReserve,
    /// A reserve price was supplied where it has no meaning: on a limit
    /// order (it already has a price) or a market ask (the seller locks
    /// base, not quote).
    UnexpectedReserve,
    /// The order's (or move's) commitment overflows `u64` ticks. Rejected
    /// instead of wrapping into an underfunded lock.
    OrderTooLarge,
    /// A bid move's new cost exceeds what its owner can fund, counting the
    /// free balance plus the order's own (recycled) lock.
    InsufficientForMove {
        /// Quote ticks the new price would have to lock.
        required: u64,
        /// What was available: free balance + this order's current lock.
        available: u64,
    },
    /// An invalid fee schedule was supplied to a constructor. Separate from
    /// [`RiskError`] because fees are engine configuration, not an
    /// operation on an account.
    Fee(FeeError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Book(error) => write!(f, "book rejected the operation: {error}"),
            Self::Risk(error) => write!(f, "risk rejected the operation: {error}"),
            Self::MarketBidRequiresReserve => {
                write!(f, "a market bid requires a reserve price to fund its lock")
            }
            Self::UnexpectedReserve => write!(f, "only market bids accept a reserve price"),
            Self::OrderTooLarge => write!(f, "order commitment overflows u64 ticks"),
            Self::InsufficientForMove {
                required,
                available,
            } => write!(
                f,
                "move needs {required} ticks but only {available} (free + this order's lock) is available"
            ),
            Self::Fee(error) => write!(f, "invalid fee schedule: {error}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<RiskError> for EngineError {
    fn from(error: RiskError) -> Self {
        Self::Risk(error)
    }
}

impl From<BookError> for EngineError {
    fn from(error: BookError) -> Self {
        Self::Book(error)
    }
}

/// The engine's own record of one live order — the money and lifecycle
/// state the book deliberately does not carry.
///
/// Every field is tracked per order (not re-derived) so fills can debit
/// incrementally and cancels can release exactly what remains. Between
/// engine operations, the live map holds **exactly the resting GTC orders**
/// — IOC/FOK/market takers join for the duration of one `place` and are
/// gone before it returns, fully filled or dead.
///
/// `price` is `None` only for a transient market-ask taker (a market order
/// has no price by construction); resting orders always carry one.
#[derive(Debug, Clone, Copy)]
pub struct LiveOrder {
    /// Owning participant.
    pub user: UserId,
    /// Book side.
    pub side: Side,
    /// Effective limit price (reserve for a rewritten market bid);
    /// `None` only for a transient market-ask taker.
    pub price: Option<Price>,
    /// Unfilled quantity.
    pub remaining: Qty,
    /// The currency this order draws on: quote for bids, base for asks.
    pub currency: CurrencyId,
    /// Funds still locked for this order (bids: un-settled quote ticks,
    /// dust included; asks: un-delivered base lots).
    pub locked: u64,
}

/// The exchange engine for one symbol: the book (matching), the ledger
/// (balances), and the live-order map (lifecycle + money state) under one
/// owner — which is what makes `place → lock → settle` atomic.
///
/// One currency pair per engine: `quote` is what bids lock and asks
/// receive; `base` is what asks lock and bids receive. Every fill settles
/// both legs, so the pair is only ever a naming decision (Phase 3 binds it
/// to the symbol specification).
///
/// The engine carries a [`FeeSchedule`] (decision row 52): [`Engine::new`]
/// installs the zero schedule — byte-identical to pre-fee behavior, which
/// keeps every bench digest comparable — and [`Engine::with_fees`] opts a
/// venue engine into maker/taker fees charged in the received asset. The
/// schedule is engine *configuration*: the journal slice will record it in
/// the journal header so replay reconstructs it.
#[derive(Debug)]
pub struct Engine {
    symbol: SymbolId,
    quote: CurrencyId,
    base: CurrencyId,
    book: OrderBook,
    ledger: Ledger,
    live: HashMap<OrderId, LiveOrder>,
    fees: FeeSchedule,
}

impl Engine {
    /// Create an empty engine: one book, one ledger, no live orders, and
    /// the **zero fee schedule** — every pre-fee behavior preserved exactly
    /// (bench digests stay comparable; see [`Engine::with_fees`]).
    #[must_use]
    pub fn new(symbol: SymbolId, quote: CurrencyId, base: CurrencyId) -> Self {
        Self {
            symbol,
            quote,
            base,
            book: OrderBook::new(symbol),
            ledger: Ledger::new(),
            live: HashMap::new(),
            fees: FeeSchedule::zero(),
        }
    }

    /// Create an engine that charges maker/taker fees (Phase 3 decision
    /// row 52): per fill, the resting side is the **maker** and the incoming
    /// side the **taker** — the roles the book already assigns — and each
    /// side's fee is floored out of what it *receives* (buyer: base lots;
    /// seller: quote ticks), accumulating in the ledger's per-currency
    /// fee sink. Self-trades pay fees on both receipts (no special case).
    ///
    /// # Errors
    /// [`FeeError`] — a schedule rate above 10_000 bps.
    pub fn with_fees(
        symbol: SymbolId,
        quote: CurrencyId,
        base: CurrencyId,
        fees: FeeSchedule,
    ) -> Self {
        Self {
            fees,
            ..Self::new(symbol, quote, base)
        }
    }

    /// The engine's fee schedule (read-only; the venue reads it for
    /// display, the journal will record it for replay).
    #[must_use]
    pub fn fee_schedule(&self) -> &FeeSchedule {
        &self.fees
    }

    /// The symbol this engine trades.
    #[must_use]
    pub fn symbol(&self) -> SymbolId {
        self.symbol
    }

    /// Read-only view of the book (best prices, depth, resting quantity).
    #[must_use]
    pub fn book(&self) -> &OrderBook {
        &self.book
    }

    /// Read-only view of the ledger (free/locked balances for checks and
    /// property tests).
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Mutable ledger access for external funding — deposits and
    /// withdrawals are outside the engine's operation model (they are how
    /// money enters and leaves the exchange, not an order operation).
    #[must_use]
    pub fn ledger_mut(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    /// The live-order map: every order the engine currently tracks. Between
    /// operations this is exactly the resting GTC set — a cross-check the
    /// property suite asserts against [`Engine::book`]'s resting count.
    #[must_use]
    pub fn live_orders(&self) -> &HashMap<OrderId, LiveOrder> {
        &self.live
    }

    /// The quote currency of this engine's pair.
    #[must_use]
    pub const fn quote(&self) -> CurrencyId {
        self.quote
    }

    /// The base currency of this engine's pair.
    #[must_use]
    pub const fn base(&self) -> CurrencyId {
        self.base
    }

    /// The engine's full observable state as `(user, currency, free,
    /// locked)` rows, sorted — the journal's replay digest folds all of it,
    /// because aggregate conservation cannot see per-account drift.
    #[must_use]
    pub fn accounts(&self) -> Vec<(UserId, CurrencyId, u64, u64)> {
        self.ledger.accounts()
    }

    /// Submit a new order: fund it, match it, settle the fills, rest or
    /// kill the remainder — atomically.
    ///
    /// `reserve` is required for a **market bid** (the order is rewritten
    /// to a Limit-IOC at that price) and must be `None` everywhere else.
    ///
    /// # Errors
    /// - [`EngineError::Book`] — the book rejected the order; any funds
    ///   already committed for it are released first (the saga's
    ///   compensation), so balances are untouched;
    /// - [`EngineError::Risk`] — the commitment exceeds the free balance;
    ///   the order never reaches the book;
    /// - [`EngineError::MarketBidRequiresReserve`] / [`EngineError::UnexpectedReserve`]
    ///   — reserve misuse;
    /// - [`EngineError::OrderTooLarge`] — the commitment overflows ticks.
    pub fn place(
        &mut self,
        mut order: Order,
        reserve: Option<Price>,
    ) -> Result<EngineOutcome, EngineError> {
        // Caller-bug gate: the engine's currency mapping is per-symbol, so
        // a mismatched order is invalid input for the money math entirely —
        // reject at the door, before anything moves. (Duplicate ids
        // deliberately do NOT get this treatment: they exercise the
        // commit → compensate saga below, which is the point of the test
        // for that path.)
        if order.symbol != self.symbol {
            return Err(EngineError::Book(BookError::SymbolMismatch {
                expected: self.symbol,
                received: order.symbol,
            }));
        }

        // Reserve rules + the market-bid rewrite (decision 2 in the module
        // docs). `effective` is the price the commitment is computed
        // against: the limit price, the reserve, or none for a market ask.
        let effective: Option<Price> = match order.order_type {
            OrderType::Market => match order.side {
                Side::Bid => {
                    let reserve = reserve.ok_or(EngineError::MarketBidRequiresReserve)?;
                    // The rewrite: a market bid *is* a Limit-IOC at its
                    // reserve — sweep up to it, kill the remainder, never
                    // rest. The book then enforces the bound on every fill.
                    // "Never rest" is structural, not incidental: the domain
                    // (`OrderType::for_tif`) only ever lets a Market order be
                    // constructed with Ioc, so the rewritten limit inherits
                    // Ioc and cannot rest even if the caller asked for GTC —
                    // they couldn't have.
                    order.order_type = OrderType::Limit { price: reserve };
                    Some(reserve)
                }
                Side::Ask => {
                    if reserve.is_some() {
                        return Err(EngineError::UnexpectedReserve);
                    }
                    None
                }
            },
            OrderType::Limit { price } => {
                if reserve.is_some() {
                    return Err(EngineError::UnexpectedReserve);
                }
                Some(price)
            }
        };

        // The commitment (saga step 1): bids lock quote ticks — ceil over
        // the whole order at the effective price; asks lock base lots —
        // exact by construction. Overflow is a rejection, never a wrap.
        let (currency, amount) = match order.side {
            Side::Bid => {
                let price = effective.expect("bids always carry a price after the rewrite");
                (
                    self.quote,
                    quote_cost_ticks(order.quantity, price).ok_or(EngineError::OrderTooLarge)?,
                )
            }
            Side::Ask => (self.base, order.quantity.lot()),
        };
        self.ledger.commit(order.user, currency, amount)?;

        // Saga step 2: hand the order to the book. On rejection, release
        // exactly what step 1 committed — the failure is invisible to the
        // ledger. (The release cannot fail: nothing has settled between.)
        let PlaceOutcome { fills, resting } = match self.book.place(order) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.ledger.release(order.user, currency, amount);
                return Err(EngineError::Book(error));
            }
        };

        // Saga step 3: the taker joins the live map, then every fill
        // settles both legs and debits both participants' tracked state.
        self.live.insert(
            order.id,
            LiveOrder {
                user: order.user,
                side: order.side,
                price: effective,
                remaining: order.quantity,
                currency,
                locked: amount,
            },
        );
        for fill in &fills {
            self.apply_fill(fill, order.side);
        }

        // A killed FOK never debited anything: its entry still holds the
        // whole lock. Release it and report the kill distinctly.
        if order.tif == TimeInForce::Fok && fills.is_empty() {
            let live = self
                .live
                .remove(&order.id)
                .expect("a killed FOK was never debited");
            self.ledger.release(live.user, live.currency, live.locked);
            return Ok(EngineOutcome::KilledFok);
        }

        // No entry left: the taker fully filled during the sweep — its dust
        // (if any) was released by the final debit. Nothing more to do.
        let Some(live) = self.live.remove(&order.id) else {
            debug_assert!(resting.is_none(), "a fully filled order cannot rest");
            return Ok(EngineOutcome::Executed {
                fills,
                resting: None,
            });
        };

        match resting {
            Some(resting_qty) => {
                debug_assert_eq!(
                    live.remaining, resting_qty,
                    "engine and book must agree on the resting quantity"
                );
                // GTC remainder rests: the entry stays, now carrying the
                // reduced remaining quantity and the un-settled lock.
                self.live.insert(order.id, live);
                Ok(EngineOutcome::Executed {
                    fills,
                    resting: Some(resting_qty),
                })
            }
            None => {
                // IOC / market remainder dies here: release what the fills
                // didn't settle — the unfilled portion plus rounding dust.
                self.ledger.release(live.user, live.currency, live.locked);
                Ok(EngineOutcome::Executed {
                    fills,
                    resting: None,
                })
            }
        }
    }

    /// Cancel a resting order and release its whole remaining lock —
    /// rounding dust included.
    ///
    /// # Errors
    /// [`EngineError::Book`] (unknown order) — the book is the authority on
    /// what rests; on error nothing has changed anywhere.
    pub fn cancel(&mut self, id: OrderId) -> Result<Qty, EngineError> {
        let removed = self.book.cancel(id).map_err(EngineError::Book)?;
        let live = self
            .live
            .remove(&id)
            .expect("engine and book track the same live orders");
        debug_assert_eq!(
            removed, live.remaining,
            "book and engine agree on what was resting"
        );
        self.ledger.release(live.user, live.currency, live.locked);
        Ok(removed)
    }

    /// Reduce a resting order's size by `by` lots — the exchange-core
    /// `reduceOrder` (adopt-deferred from the Phase 2 re-read; landed in
    /// Phase 3 where the money leg and the reduce event belong).
    ///
    /// Semantics (decision row 55): `by` is clamped to the remaining
    /// quantity, so `by ≥ remaining` **is** a cancel — the whole lock
    /// (rounding dust included) releases and the order leaves book, live
    /// map, and index. A partial reduce keeps queue position and releases
    /// the lock **the way a fill of the same size would have**: a bid
    /// releases `floor(reduced × price)` quote ticks, an ask releases
    /// exactly `reduced` base lots. Floor subadditivity guarantees the
    /// remaining lock still covers the remaining obligation for ANY split
    /// of the original size; the bid's ceiling dust stays with the
    /// remainder and returns at order death — the same convention
    /// settlement uses. No fills occur, so no fee legs and no counterparty
    /// state is touched.
    ///
    /// # Errors
    /// [`EngineError::Book`] ([`BookError::UnknownOrder`]) if the id is not
    /// resting; on error nothing has changed anywhere.
    pub fn reduce_order(&mut self, id: OrderId, by: Qty) -> Result<ReduceOutcome, EngineError> {
        let outcome = self.book.reduce(id, by).map_err(EngineError::Book)?;
        let live = self
            .live
            .get_mut(&id)
            .expect("engine and book track the same live orders");
        debug_assert!(
            outcome.reduced.lot() <= live.remaining.lot(),
            "the book cannot reduce more than the engine tracks"
        );
        let release = if outcome.removed {
            // Fully reduced = a cancel: the whole lock goes back, dust
            // included — there is no remainder left to hold the obligation.
            live.locked
        } else {
            match live.side {
                Side::Bid => {
                    let price = live.price.expect("resting bids carry a price");
                    quote_cost_floor_ticks(outcome.reduced, price).expect(
                        "reduced × price fit in u64 when the order was placed (it is a sub-quantity)",
                    )
                }
                Side::Ask => outcome.reduced.lot(),
            }
        };
        live.remaining = live
            .remaining
            .checked_sub(outcome.reduced)
            .expect("the book never reduces more than the tracked remainder");
        live.locked = live
            .locked
            .checked_sub(release)
            .expect("Σ floor releases ≤ the ceil lock (floor subadditivity over the splits)");
        self.ledger.release(live.user, live.currency, release);
        if outcome.removed {
            let live = self.live.remove(&id).expect("entry just mutated");
            debug_assert_eq!(
                live.locked, 0,
                "a fully reduced order released its whole lock"
            );
            debug_assert_eq!(live.remaining.lot(), 0);
        }
        Ok(outcome)
    }

    /// Reprice a resting order (the exchange-core `move` operation).
    ///
    /// For a **bid**, the lock is recomputed at the new price: the funding
    /// check counts free funds *plus this order's own recycled lock*, the
    /// book gets to reject a crossing move *before* any money moves, and
    /// only then does the old lock release and the new one commit (which
    /// cannot fail given the check). For an **ask**, the lock is base lots
    /// — price-independent — so no money moves at all.
    ///
    /// # Errors
    /// - [`EngineError::Book`] — unknown order, or the new price would
    ///   lock/cross the book (rejected with balances untouched);
    /// - [`EngineError::InsufficientForMove`] — the top-up doesn't fit;
    /// - [`EngineError::OrderTooLarge`] — the new commitment overflows.
    pub fn move_order(&mut self, id: OrderId, new_price: Price) -> Result<(), EngineError> {
        let Some(live) = self.live.get(&id).copied() else {
            return Err(EngineError::Book(BookError::UnknownOrder { id }));
        };

        match live.side {
            Side::Ask => {
                self.book
                    .move_order(id, new_price)
                    .map_err(EngineError::Book)?;
                self.live.get_mut(&id).expect("entry just read").price = Some(new_price);
                Ok(())
            }
            Side::Bid => {
                // New lock = ceil over the SAME remaining quantity at the
                // new price. Overflow → reject before anything moves.
                let new_cost = quote_cost_ticks(live.remaining, new_price)
                    .ok_or(EngineError::OrderTooLarge)?;
                // Funding check with the money this order already holds:
                // free plus its own lock. Other orders' locks are theirs.
                let available = self
                    .ledger
                    .free(live.user, live.currency)
                    .checked_add(live.locked)
                    .expect("free + this order's lock cannot overflow: the lock came out of free");
                if available < new_cost {
                    return Err(EngineError::InsufficientForMove {
                        required: new_cost,
                        available,
                    });
                }
                // Position-limit pre-check (decision row 54): the move
                // top-up below is release-then-commit, which must never
                // fail mid-way (a failed commit after release would leave
                // the order lockless). The cap check replicates commit's
                // rule for the post-release state: the account's lock
                // WITHOUT this order's current lock, plus the new cost.
                if let Some(cap) = self.ledger.position_limit(live.user, live.currency) {
                    let locked_without_this = self
                        .ledger
                        .locked(live.user, live.currency)
                        .checked_sub(live.locked)
                        .expect("this order's own lock is part of the account's locked total");
                    let would_lock = locked_without_this
                        .checked_add(new_cost)
                        .expect("same sum commit itself computes");
                    if would_lock > cap {
                        return Err(EngineError::Risk(RiskError::PositionLimitExceeded {
                            currency: live.currency,
                            cap,
                            would_lock,
                        }));
                    }
                }
                // The book's WouldCross rejection happens here — before any
                // money moves — so a rejected move leaves balances alone.
                self.book
                    .move_order(id, new_price)
                    .map_err(EngineError::Book)?;
                // Release the old lock, commit the new. The commit cannot
                // fail: free just absorbed the old lock, and we checked
                // free + old ≥ new above.
                self.ledger.release(live.user, live.currency, live.locked);
                self.ledger
                    .commit(live.user, live.currency, new_cost)
                    .expect("funding checked above against free + the recycled lock");
                let updated = self.live.get_mut(&id).expect("entry just read");
                updated.price = Some(new_price);
                updated.locked = new_cost;
                Ok(())
            }
        }
    }

    /// Settle one fill: both legs, both participants.
    ///
    /// The bid side pays `floor(fill_qty × fill_price)` quote **from its
    /// lock**; the ask side pays `fill_qty` base lots **from its lock**;
    /// each side receives the other currency as free funds. Both amounts
    /// derive from the fill itself, so what leaves one lock arrives as the
    /// other side's free funds — conservation is structural, per fill.
    /// Self-trade (same user on both sides) is already correct in the
    /// ledger: `settle` returns the lock to the same account's free funds.
    ///
    /// The maker's order is always tracked (every resting order went
    /// through `place`), and the taker joined the live map just before the
    /// fill loop — so both lookups below are engine invariants, and a miss
    /// is a bug to panic on, not an input to reject.
    ///
    /// The fee legs (Phase 3, decision row 52) run **after** both settle
    /// legs: each side's fee is floored out of what it just received —
    /// the maker's on its receipt, the taker's on its own — and moved to
    /// the ledger's fee sink. Fees never touch any lock: the lock covers
    /// the *paid* asset, the fee comes from the *received* asset, which no
    /// `LiveOrder` tracks — so locks, [`LiveOrder`] state, and every lock-
    /// sufficiency invariant are untouched by fees by construction. A zero
    /// schedule collects nothing (`collect_fee` no-ops on 0), making the
    /// 0-fee path byte-identical to the pre-fee engine.
    fn apply_fill(&mut self, fill: &Fill, taker_side: Side) {
        debug_assert_ne!(
            fill.maker_order_id, fill.taker_order_id,
            "an order can never be its own maker: the book rejects duplicate live ids"
        );
        let quote_amount = quote_cost_floor_ticks(fill.quantity, fill.price).expect(
            "fill cost is bounded by the payer's own place-time lock math (see quote_cost_floor_ticks)",
        );
        // Who pays which currency: the bid side pays quote, the ask side
        // pays base — regardless of which one is maker or taker.
        let (quote_payer, base_payer) = match taker_side {
            Side::Bid => (fill.taker_user, fill.maker_user),
            Side::Ask => (fill.maker_user, fill.taker_user),
        };
        let (quote, base) = (self.quote, self.base);
        self.debit_live(fill.maker_order_id, fill.quantity, quote_amount);
        self.debit_live(fill.taker_order_id, fill.quantity, quote_amount);
        self.ledger
            .settle(quote_payer, base_payer, quote, quote_amount);
        self.ledger
            .settle(base_payer, quote_payer, base, fill.quantity.lot());

        // Fee legs — the maker (resting) pays maker_bps on what it received,
        // the taker (incoming) pays taker_bps on its own receipt — each
        // charged in the received asset (decision row 52): the seller's fee
        // comes out of its quote proceeds, the buyer's out of its base lots.
        // Dispatch on `taker_side`, not user equality: a self-trade puts the
        // same user on both sides of the fill, and only the side tells us
        // which receipt is the taker order's.
        let base_receiver = match taker_side {
            Side::Bid => fill.taker_user,
            Side::Ask => fill.maker_user,
        };
        let base_fee = match taker_side {
            Side::Bid => self.fees.taker_fee(fill.quantity.lot()),
            Side::Ask => self.fees.maker_fee(fill.quantity.lot()),
        };
        self.ledger.collect_fee(base_receiver, base, base_fee);
        // Quote-side receipts: the bid side received `quote_amount` ticks.
        let quote_receiver = match taker_side {
            Side::Bid => fill.maker_user,
            Side::Ask => fill.taker_user,
        };
        let quote_fee = match taker_side {
            Side::Bid => self.fees.maker_fee(quote_amount),
            Side::Ask => self.fees.taker_fee(quote_amount),
        };
        self.ledger.collect_fee(quote_receiver, quote, quote_fee);
    }

    /// Debit one participant's live-order state for a fill: remaining
    /// quantity and lock both shrink; an exhausted order leaves the map and
    /// releases its leftover lock (the bid-side rounding dust, or exactly
    /// zero for an ask).
    ///
    /// The order of operations matters and is proven safe: the dust
    /// released here is `lock − Σ settled so far` *including this fill*, so
    /// the ledger still holds exactly this fill's settlement when the
    /// `settle` calls in [`Engine::apply_fill`] draw it — the lock reaches
    /// zero, never negative.
    fn debit_live(&mut self, id: OrderId, fill_qty: Qty, quote_amount: u64) {
        let Some(live) = self.live.get_mut(&id) else {
            panic!("engine invariant violated: fill participant {id:?} is not tracked");
        };
        live.remaining = live
            .remaining
            .checked_sub(fill_qty)
            .expect("a fill never exceeds a participant's remaining quantity");
        match live.side {
            Side::Bid => {
                live.locked = live
                    .locked
                    .checked_sub(quote_amount)
                    .expect("Σ per-fill floors ≤ the ceil lock (floor subadditivity)");
            }
            Side::Ask => {
                live.locked = live
                    .locked
                    .checked_sub(fill_qty.lot())
                    .expect("an ask's lock is exactly its remaining lots");
            }
        }
        if live.remaining.lot() == 0 {
            let live = self.live.remove(&id).expect("entry just read");
            self.ledger.release(live.user, live.currency, live.locked);
        }
    }
}

#[cfg(test)]
mod tests;
