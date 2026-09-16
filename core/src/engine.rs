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

use crate::book::{BookError, Fill, OrderBook, PlaceOutcome};
use crate::domain::{
    CurrencyId, Order, OrderId, OrderType, Price, Qty, Side, SymbolId, TimeInForce, UserId,
};
use crate::risk::{FeeSchedule, Ledger, RiskError, quote_cost_floor_ticks, quote_cost_ticks};

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
mod tests {
    use super::*;
    use crate::domain::QTY_SCALE;

    const USD: CurrencyId = CurrencyId(1); // quote
    const BTC: CurrencyId = CurrencyId(2); // base
    const ALICE: UserId = UserId(1);
    const BOB: UserId = UserId(2);

    // ---- Builders (tests may unwrap; shipped code may not) --------------

    fn engine() -> Engine {
        Engine::new(SymbolId(1), USD, BTC)
    }

    /// Both users funded identically, so every test can assert exact
    /// balances: 20,000.0 quote and 5.0 base each.
    fn funded() -> Engine {
        let mut engine = engine();
        for user in [ALICE, BOB] {
            engine
                .ledger_mut()
                .deposit(user, USD, 200_000_000_000)
                .unwrap();
            engine.ledger_mut().deposit(user, BTC, 500_000_000).unwrap();
        }
        engine
    }

    fn limit(price_ticks: u64) -> OrderType {
        OrderType::Limit {
            price: Price::from_ticks(price_ticks).unwrap(),
        }
    }

    /// GTC limit order with id == timestamp, the usual test shape.
    fn gtc(id: u64, user: UserId, side: Side, price_ticks: u64, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            user,
            SymbolId(1),
            side,
            limit(price_ticks),
            TimeInForce::Gtc,
            Qty::from_lots(lots).unwrap(),
            id,
        )
        .unwrap()
    }

    fn ioc(id: u64, user: UserId, side: Side, price_ticks: u64, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            user,
            SymbolId(1),
            side,
            limit(price_ticks),
            TimeInForce::Ioc,
            Qty::from_lots(lots).unwrap(),
            id,
        )
        .unwrap()
    }

    fn fok_bid(id: u64, user: UserId, price_ticks: u64, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            user,
            SymbolId(1),
            Side::Bid,
            limit(price_ticks),
            TimeInForce::Fok,
            Qty::from_lots(lots).unwrap(),
            id,
        )
        .unwrap()
    }

    fn market_bid(id: u64, user: UserId, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            user,
            SymbolId(1),
            Side::Bid,
            OrderType::Market,
            TimeInForce::Ioc, // market is IOC-only by domain rule
            Qty::from_lots(lots).unwrap(),
            id,
        )
        .unwrap()
    }

    fn market_ask(id: u64, user: UserId, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            user,
            SymbolId(1),
            Side::Ask,
            OrderType::Market,
            TimeInForce::Ioc,
            Qty::from_lots(lots).unwrap(),
            id,
        )
        .unwrap()
    }

    // ---- Full lifecycle -------------------------------------------------

    #[test]
    fn full_lifecycle_through_the_engine() {
        let mut engine = funded();

        // Alice asks 2.0 base @ 100.0: locks 200,000,000 base lots.
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_000_000, 200_000_000), None)
            .unwrap();
        assert_eq!(engine.ledger().locked(ALICE, BTC), 200_000_000);
        assert_eq!(engine.ledger().free(ALICE, BTC), 300_000_000);

        // Bob bids 1.0 base @ 100.0: locks ceil(100M × 1M / 1e8) = 1,000,000
        // ticks — exact, no dust. The sweep fills and settles both legs.
        let outcome = engine
            .place(gtc(2, BOB, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();
        let EngineOutcome::Executed { fills, resting } = outcome else {
            panic!("a crossing GTC bid is executed, not killed");
        };
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].maker_order_id, OrderId(1));
        assert_eq!(fills[0].taker_order_id, OrderId(2));
        assert_eq!(fills[0].price, Price::from_ticks(1_000_000).unwrap()); // maker's price
        assert_eq!(fills[0].quantity, Qty::from_lots(100_000_000).unwrap());
        assert_eq!(resting, None); // fully filled

        // Alice: 1.0 sold (quote received free), 1.0 still locked.
        assert_eq!(engine.ledger().free(ALICE, USD), 200_001_000_000);
        assert_eq!(engine.ledger().locked(ALICE, BTC), 100_000_000);
        // Bob: quote lock settled away, base received free.
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
        assert_eq!(engine.ledger().free(BOB, USD), 199_999_000_000);
        assert_eq!(engine.ledger().free(BOB, BTC), 600_000_000);

        // Conservation, the outside view: trading moved value, never minted.
        let total = |currency: CurrencyId| {
            [ALICE, BOB]
                .iter()
                .map(|u| engine.ledger().free(*u, currency) + engine.ledger().locked(*u, currency))
                .sum::<u64>()
        };
        assert_eq!(total(USD), 400_000_000_000);
        assert_eq!(total(BTC), 1_000_000_000);

        // Cancel the resting remainder: the whole remaining lock releases.
        engine.cancel(OrderId(1)).unwrap();
        assert_eq!(engine.ledger().locked(ALICE, BTC), 0);
        assert_eq!(engine.ledger().free(ALICE, BTC), 400_000_000);
    }

    // ---- The settlement-rounding regression (decision log 2026-09-07) ---

    #[test]
    fn dusty_taker_bid_settles_floor_and_releases_dust() {
        // THE rounding trap: an order of 3 lots @ 3 ticks locks
        // ceil(9 / 1e8) = 1 tick. Split across three 1-lot fills,
        // per-fill CEIL would settle 1+1+1 = 3 > 1 and panic the ledger's
        // `settle` at the second fill. Per-fill FLOOR settles 0 each and
        // the un-settled dust returns to free on completion. If this test
        // ever panics, someone reintroduced ceil settlement.
        let mut engine = funded();
        for id in [1, 2, 3] {
            engine.place(gtc(id, ALICE, Side::Ask, 3, 1), None).unwrap();
        }
        let outcome = engine.place(gtc(4, BOB, Side::Bid, 3, 3), None).unwrap();
        let EngineOutcome::Executed {
            fills,
            resting: None,
        } = outcome
        else {
            panic!("the sweep fully fills the dusty bid");
        };
        assert_eq!(fills.len(), 3);

        // Every fill settled floor(1 lot × 3 ticks / 1e8) = 0 quote — the
        // sellers' sub-tick haircut on a dusty fill.
        assert_eq!(engine.ledger().free(ALICE, USD), 200_000_000_000);
        assert_eq!(
            engine.ledger().locked(ALICE, BTC),
            0,
            "all three asks consumed"
        );
        assert_eq!(
            engine.ledger().free(ALICE, BTC),
            499_999_997,
            "3 locked lots delivered"
        );
        // Bob received 3 lots for 0 quote; his 1-tick lock is released.
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
        assert_eq!(engine.ledger().free(BOB, BTC), 500_000_003);
        assert!(engine.live_orders().is_empty());
    }

    #[test]
    fn dusty_resting_bid_swept_in_three_fills_releases_dust() {
        // The mirrored case: the DUSTY order is the resting maker. Its lock
        // must survive three floor-settled fills, then return whole.
        let mut engine = funded();
        engine.place(gtc(1, BOB, Side::Bid, 3, 3), None).unwrap();
        assert_eq!(
            engine.ledger().locked(BOB, USD),
            1,
            "ceil dust locked with the order"
        );

        for id in [2, 3, 4] {
            let outcome = engine.place(ioc(id, ALICE, Side::Ask, 3, 1), None).unwrap();
            let EngineOutcome::Executed {
                fills,
                resting: None,
            } = outcome
            else {
                panic!("each IOC exactly fills one lot");
            };
            assert_eq!(fills.len(), 1);
        }

        // Every fill settled floor(1 lot × 3 ticks / 1e8) = 0 quote from
        // Bob's lock; the order is exhausted, so the 1-tick dust returns
        // to free. (The mid-life lock-sufficiency check is the next test.)

        assert_eq!(
            engine.ledger().locked(BOB, USD),
            0,
            "dust released at exhaustion"
        );
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);
        assert_eq!(
            engine.ledger().free(BOB, BTC),
            500_000_003,
            "3 lots received"
        );
        assert_eq!(engine.ledger().locked(ALICE, BTC), 0);
        assert_eq!(
            engine.ledger().free(ALICE, BTC),
            499_999_997,
            "3 locked lots delivered"
        );
        assert_eq!(engine.ledger().free(ALICE, USD), 200_000_000_000);
    }

    #[test]
    fn partial_dusty_fill_keeps_lock_over_remaining_obligation() {
        // The per-order invariant, mid-life: after a floor-settled partial
        // fill, the remaining lock must still cover the exact remaining
        // obligation floor(remaining × price / QTY_SCALE).
        let mut engine = funded();
        engine.place(gtc(1, BOB, Side::Bid, 3, 3), None).unwrap(); // lock 1 tick
        engine.place(ioc(2, ALICE, Side::Ask, 3, 1), None).unwrap(); // settles 0

        let live = engine
            .live_orders()
            .get(&OrderId(1))
            .expect("bid still resting");
        assert_eq!(live.remaining, Qty::from_lots(2).unwrap());
        assert_eq!(engine.ledger().locked(BOB, USD), 1);
        let obligation = live.remaining.lot() * live.price.unwrap().tick() / QTY_SCALE;
        assert!(engine.ledger().locked(BOB, USD) >= obligation);
    }

    // ---- FOK: kill vs fill ------------------------------------------------

    #[test]
    fn fok_kill_releases_the_whole_lock() {
        let mut engine = funded();
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_000_000, 5), None)
            .unwrap(); // 5 lots @ 100

        // FOK for 10M lots (0.1 base): only 5 lots available → killed, and
        // the whole lock (ceil(10M × 1M / 1e8) = 100,000 ticks) is back in
        // free.
        let outcome = engine
            .place(fok_bid(2, BOB, 1_000_000, 10_000_000), None)
            .unwrap();
        assert_eq!(outcome, EngineOutcome::KilledFok);
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);
        // The book was never touched by the killed order.
        assert_eq!(engine.book().len(), 1);
        assert_eq!(
            engine.book().best_ask(),
            Some(Price::from_ticks(1_000_000).unwrap())
        );
        // Only the maker's resting ask remains tracked — the killed FOK's
        // own entry left with its full lock.
        assert_eq!(engine.live_orders().len(), 1);
    }

    #[test]
    fn fok_that_fills_is_executed_not_killed() {
        let mut engine = funded();
        // 5M lots (0.05 base) @ 100.0: commitment ceil(5M × 1M / 1e8) =
        // 50,000 ticks — exact, no dust, so this test isolates the fill
        // path from the rounding paths (the dusty tests cover those).
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_000_000, 5_000_000), None)
            .unwrap();

        let outcome = engine
            .place(fok_bid(2, BOB, 1_000_000, 5_000_000), None)
            .unwrap();
        let EngineOutcome::Executed {
            fills,
            resting: None,
        } = outcome
        else {
            panic!("an exactly-matching FOK executes");
        };
        assert_eq!(fills.len(), 1);
        // Lock settled away exactly: 50,000 ticks moved to Alice.
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
        assert_eq!(engine.ledger().free(BOB, USD), 199_999_950_000);
        assert_eq!(engine.ledger().free(BOB, BTC), 505_000_000);
        assert_eq!(engine.ledger().free(ALICE, USD), 200_000_050_000);
        assert_eq!(engine.ledger().free(ALICE, BTC), 495_000_000);
    }

    // ---- The risk gate -----------------------------------------------------

    #[test]
    fn unfundable_order_never_reaches_the_book() {
        let mut engine = engine();
        engine.ledger_mut().deposit(BOB, USD, 1_000_000).unwrap(); // 100.0 quote

        // A bid for 2.0 base @ 100.0 needs 200.0 quote — more than free.
        let error = engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 200_000_000), None)
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::Risk(RiskError::InsufficientForOrder { .. })
        ));
        // Nothing happened anywhere: book empty, no live order, balance
        // untouched (all-or-nothing commit).
        assert!(engine.book().is_empty());
        assert!(engine.live_orders().is_empty());
        assert_eq!(engine.ledger().free(BOB, USD), 1_000_000);
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
    }

    #[test]
    fn order_too_large_is_rejected_before_anything_moves() {
        let mut engine = funded();
        // u64::MAX lots × u64::MAX ticks cannot fit u64 ticks — the
        // commitment overflows and is rejected, not wrapped into a
        // cheap-looking lock.
        let error = engine
            .place(gtc(1, BOB, Side::Bid, u64::MAX, u64::MAX), None)
            .unwrap_err();
        assert_eq!(error, EngineError::OrderTooLarge);
        assert!(engine.book().is_empty());
        assert!(engine.live_orders().is_empty());
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
    }

    // ---- The place saga's compensation -------------------------------------

    #[test]
    fn duplicate_id_compensates_the_commit() {
        let mut engine = funded();
        // Alice rests a bid under id 1: locks 1,000,000 ticks.
        engine
            .place(gtc(1, ALICE, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();
        let (alice_free, alice_locked) = (
            engine.ledger().free(ALICE, USD),
            engine.ledger().locked(ALICE, USD),
        );

        // Bob reuses id 1: the saga commits his 500,000-tick lock, the book
        // rejects the duplicate, and the compensation releases it — the
        // failure is invisible to the ledger.
        let error = engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 50_000_000), None)
            .unwrap_err();
        assert_eq!(
            error,
            EngineError::Book(BookError::DuplicateOrder { id: OrderId(1) })
        );
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
        // Alice's order is untouched and still resting.
        assert_eq!(engine.ledger().free(ALICE, USD), alice_free);
        assert_eq!(engine.ledger().locked(ALICE, USD), alice_locked);
        assert_eq!(engine.book().len(), 1);
    }

    // ---- Cancel ------------------------------------------------------------

    #[test]
    fn cancel_releases_full_lock_including_dust() {
        let mut engine = funded();
        // 3 lots @ 3 ticks locks ceil(9/1e8) = 1 tick of dust-cover.
        engine.place(gtc(1, BOB, Side::Bid, 3, 3), None).unwrap();
        assert_eq!(engine.ledger().locked(BOB, USD), 1);

        let removed = engine.cancel(OrderId(1)).unwrap();
        assert_eq!(removed, Qty::from_lots(3).unwrap());
        // The WHOLE lock — dust included — is back in free.
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);
        assert!(engine.book().is_empty());
        assert!(engine.live_orders().is_empty());
    }

    #[test]
    fn cancel_of_unknown_order_errors_untouched() {
        let mut engine = funded();
        engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();

        let error = engine.cancel(OrderId(99)).unwrap_err();
        assert_eq!(
            error,
            EngineError::Book(BookError::UnknownOrder { id: OrderId(99) })
        );
        // The live order and its lock are untouched.
        assert_eq!(engine.ledger().locked(BOB, USD), 1_000_000);
        assert_eq!(engine.book().len(), 1);
    }

    // ---- Move: bids relock, asks don't touch money --------------------------

    #[test]
    fn move_bid_up_requires_and_uses_top_up() {
        let mut engine = funded();
        // Bob: bid 1.0 base @ 100.0 locks 1,000,000 ticks.
        engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();

        // Move to 150.0: new cost ceil(100M × 1.5M / 1e8) = 1,500,000 —
        // the check passes against free + the recycled lock, then the old
        // lock releases and the new one commits.
        engine
            .move_order(OrderId(1), Price::from_ticks(1_500_000).unwrap())
            .unwrap();
        assert_eq!(engine.ledger().locked(BOB, USD), 1_500_000);
        assert_eq!(engine.ledger().free(BOB, USD), 199_998_500_000);
        // Book and engine agree on the new resting price.
        assert_eq!(
            engine.book().best_bid(),
            Some(Price::from_ticks(1_500_000).unwrap())
        );
        assert_eq!(
            engine.live_orders().get(&OrderId(1)).unwrap().price,
            Some(Price::from_ticks(1_500_000).unwrap())
        );
    }

    #[test]
    fn move_bid_up_insufficient_rejects_untouched() {
        let mut engine = engine();
        engine.ledger_mut().deposit(BOB, USD, 1_200_000).unwrap();
        engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();

        // free 200,000 + own lock 1,000,000 = 1,200,000 < 1,500,000 needed.
        let error = engine
            .move_order(OrderId(1), Price::from_ticks(1_500_000).unwrap())
            .unwrap_err();
        assert_eq!(
            error,
            EngineError::InsufficientForMove {
                required: 1_500_000,
                available: 1_200_000,
            }
        );
        // Nothing moved: balance, lock, book price.
        assert_eq!(engine.ledger().free(BOB, USD), 200_000);
        assert_eq!(engine.ledger().locked(BOB, USD), 1_000_000);
        assert_eq!(
            engine.book().best_bid(),
            Some(Price::from_ticks(1_000_000).unwrap())
        );
    }

    #[test]
    fn move_bid_down_releases_the_difference() {
        let mut engine = funded();
        engine
            .place(gtc(1, BOB, Side::Bid, 1_500_000, 100_000_000), None)
            .unwrap();
        assert_eq!(engine.ledger().locked(BOB, USD), 1_500_000);

        engine
            .move_order(OrderId(1), Price::from_ticks(1_000_000).unwrap())
            .unwrap();
        assert_eq!(engine.ledger().locked(BOB, USD), 1_000_000);
        assert_eq!(engine.ledger().free(BOB, USD), 199_999_000_000);
    }

    #[test]
    fn ask_move_touches_no_money() {
        let mut engine = funded();
        // Alice asks 1.0 base @ 110.0: locks 100M base lots (price-free).
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_100_000, 100_000_000), None)
            .unwrap();
        let (btc_free, btc_locked, usd_free) = (
            engine.ledger().free(ALICE, BTC),
            engine.ledger().locked(ALICE, BTC),
            engine.ledger().free(ALICE, USD),
        );

        engine
            .move_order(OrderId(1), Price::from_ticks(1_200_000).unwrap())
            .unwrap();
        assert_eq!(engine.ledger().free(ALICE, BTC), btc_free);
        assert_eq!(engine.ledger().locked(ALICE, BTC), btc_locked);
        assert_eq!(engine.ledger().free(ALICE, USD), usd_free);
        assert_eq!(
            engine.book().best_ask(),
            Some(Price::from_ticks(1_200_000).unwrap())
        );
    }

    #[test]
    fn would_cross_leaves_money_alone() {
        let mut engine = funded();
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_100_000, 100_000_000), None)
            .unwrap();
        engine
            .place(gtc(2, BOB, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();

        // Moving the bid to the ask would LOCK the book — the book rejects
        // before any money moves (funds were sufficient here, which is the
        // point: the book's rejection, not the funds check, is what fires).
        let error = engine
            .move_order(OrderId(2), Price::from_ticks(1_100_000).unwrap())
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::Book(BookError::WouldCross { id: OrderId(2), .. })
        ));
        assert_eq!(engine.ledger().free(BOB, USD), 199_999_000_000);
        assert_eq!(engine.ledger().locked(BOB, USD), 1_000_000);
        assert_eq!(
            engine.book().best_bid(),
            Some(Price::from_ticks(1_000_000).unwrap())
        );
        assert_eq!(
            engine.book().best_ask(),
            Some(Price::from_ticks(1_100_000).unwrap())
        );
    }

    #[test]
    fn move_to_overflowing_price_rejects_untouched() {
        let mut engine = funded();
        engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();

        // 100M lots × u64::MAX ticks cannot fit u64 ticks.
        let error = engine
            .move_order(OrderId(1), Price::from_ticks(u64::MAX).unwrap())
            .unwrap_err();
        assert_eq!(error, EngineError::OrderTooLarge);
        assert_eq!(engine.ledger().locked(BOB, USD), 1_000_000);
        assert_eq!(
            engine.book().best_bid(),
            Some(Price::from_ticks(1_000_000).unwrap())
        );
    }

    // ---- Market orders: the reserve rewrite ----------------------------------

    #[test]
    fn market_bid_requires_reserve() {
        let mut engine = funded();

        // No reserve on a market bid: nothing to lock against — rejected.
        let error = engine
            .place(market_bid(1, BOB, 100_000_000), None)
            .unwrap_err();
        assert_eq!(error, EngineError::MarketBidRequiresReserve);
        assert!(engine.book().is_empty());
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);

        // A limit order already has its price — a reserve is meaningless.
        let error = engine
            .place(
                gtc(1, BOB, Side::Bid, 1_000_000, 100_000_000),
                Some(Price::from_ticks(1_000_000).unwrap()),
            )
            .unwrap_err();
        assert_eq!(error, EngineError::UnexpectedReserve);

        // A market ask locks base, never quote — a reserve is meaningless.
        let error = engine
            .place(
                market_ask(1, ALICE, 100_000_000),
                Some(Price::from_ticks(1_000_000).unwrap()),
            )
            .unwrap_err();
        assert_eq!(error, EngineError::UnexpectedReserve);
    }

    #[test]
    fn market_bid_with_reserve_sweeps_and_releases_leftover() {
        let mut engine = funded();
        // Alice rests 0.3 base @ 100.0.
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_000_000, 30_000_000), None)
            .unwrap();

        // Bob's market bid for 1.0 base with reserve 150.0: the rewrite
        // makes it a Limit-IOC @ 150.0, locking ceil(100M × 1.5M / 1e8) =
        // 1,500,000 ticks. It sweeps 0.3 at the maker's 100.0, settling
        // 300,000 ticks; the 0.7 remainder dies and the leftover lock
        // (1,200,000) returns to free.
        let outcome = engine
            .place(
                market_bid(2, BOB, 100_000_000),
                Some(Price::from_ticks(1_500_000).unwrap()),
            )
            .unwrap();
        let EngineOutcome::Executed {
            fills,
            resting: None,
        } = outcome
        else {
            panic!("a market order never rests");
        };
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].price, Price::from_ticks(1_000_000).unwrap());
        assert_eq!(engine.ledger().locked(BOB, USD), 0, "unused lock released");
        assert_eq!(engine.ledger().free(BOB, USD), 199_999_700_000);
        assert_eq!(engine.ledger().free(BOB, BTC), 530_000_000);
        assert_eq!(engine.ledger().free(ALICE, USD), 200_000_300_000);
        assert_eq!(engine.ledger().locked(ALICE, BTC), 0);
        assert!(engine.live_orders().is_empty(), "both orders are dead");
    }

    #[test]
    fn market_ask_sweeps_without_reserve() {
        let mut engine = funded();
        // Bob bids 0.2 base @ 100.0: locks 200,000 ticks.
        engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 20_000_000), None)
            .unwrap();

        // Alice's market ask for 0.5 base needs no reserve: she locks
        // 50M base lots, delivers 20M to the fill, and the 30M remainder
        // dies with its lock released.
        let outcome = engine
            .place(market_ask(2, ALICE, 50_000_000), None)
            .unwrap();
        let EngineOutcome::Executed {
            fills,
            resting: None,
        } = outcome
        else {
            panic!("a market order never rests");
        };
        assert_eq!(fills.len(), 1);
        assert_eq!(engine.ledger().locked(ALICE, BTC), 0);
        // 50M locked at place, 20M delivered to Bob, 30M remainder released:
        // net free = 500M − 20M delivered = 480M.
        assert_eq!(engine.ledger().free(ALICE, BTC), 480_000_000);
        assert_eq!(engine.ledger().free(ALICE, USD), 200_000_200_000);
        // Bob's bid fully filled: his quote lock settled away exactly.
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
        assert_eq!(engine.ledger().free(BOB, USD), 199_999_800_000);
        assert_eq!(engine.ledger().free(BOB, BTC), 520_000_000);
    }

    // ---- Self-trade -----------------------------------------------------------

    #[test]
    fn self_trade_conserves_both_currencies() {
        let mut engine = funded();
        // Alice crosses her own ask: both settle legs have payer == payee,
        // which must net to exactly zero movement of her own balances.
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_000_000, 100_000_000), None)
            .unwrap();
        engine
            .place(gtc(2, ALICE, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();

        // Both legs returned to the same account: quote lock settled away
        // into her own free funds, base lock likewise.
        assert_eq!(engine.ledger().free(ALICE, USD), 200_000_000_000);
        assert_eq!(engine.ledger().locked(ALICE, USD), 0);
        assert_eq!(engine.ledger().free(ALICE, BTC), 500_000_000);
        assert_eq!(engine.ledger().locked(ALICE, BTC), 0);
        // Bob was never a party; conservation holds from the outside.
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);
        assert_eq!(engine.ledger().free(BOB, BTC), 500_000_000);
        assert!(
            engine.live_orders().is_empty(),
            "both orders fully consumed"
        );
    }

    // ---- The id-lifecycle sweep (docs/TODO.md Phase 1) -----------------
    //
    // FINDING (2026-09-14): id uniqueness binds among LIVE orders only.
    // A canceled, fully-filled, or killed id leaves no trace — the book's
    // index and the engine's live map both drop it — so the id may be
    // recycled as a fresh, independent order (venue-standard ClOrdID
    // recycling). A duplicate of an id still live is rejected with full
    // saga compensation. These tests pin both halves, plus the
    // no-double-release guarantee for cancel/move aimed at dead ids.

    /// Place a GTC bid of `lots` at `price_ticks` for `user`, fully funded.
    fn resting_bid(engine: &mut Engine, id: u64, user: UserId, price_ticks: u64, lots: u64) {
        engine
            .place(gtc(id, user, Side::Bid, price_ticks, lots), None)
            .unwrap();
    }

    /// An ask that fully fills a resting bid of `lots` at the same price.
    fn filling_ask(engine: &mut Engine, id: u64, user: UserId, price_ticks: u64, lots: u64) {
        engine
            .place(gtc(id, user, Side::Ask, price_ticks, lots), None)
            .unwrap();
    }

    #[test]
    fn cancelled_id_can_be_recycled_as_a_fresh_order() {
        let mut engine = funded();
        resting_bid(&mut engine, 1, BOB, 1_000_000, 100_000_000);
        engine.cancel(OrderId(1)).unwrap();
        assert!(engine.live_orders().is_empty());

        // The id is dead: reuse is ACCEPTED as a fresh, independent order —
        // venue-standard id recycling. The recycled order runs the full
        // saga; its lock is new money, no ghost state from the predecessor.
        engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();
        assert_eq!(engine.live_orders().len(), 1);
        assert_eq!(engine.ledger().locked(BOB, USD), 1_000_000);
        assert_eq!(engine.ledger().free(BOB, USD), 199_999_000_000);

        // Uniqueness still binds among LIVE orders: a second order under
        // the now-live id is rejected and the saga compensates invisibly.
        let error = engine
            .place(gtc(1, ALICE, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap_err();
        assert_eq!(
            error,
            EngineError::Book(BookError::DuplicateOrder { id: OrderId(1) })
        );
        assert_eq!(engine.ledger().free(ALICE, USD), 200_000_000_000);

        // The recycled order is fully operational: cancel works on it once.
        engine.cancel(OrderId(1)).unwrap();
        assert!(engine.live_orders().is_empty());
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
        assert_eq!(engine.ledger().free(BOB, USD), 200_000_000_000);
    }

    #[test]
    fn fully_filled_id_can_be_recycled_as_a_fresh_order() {
        let mut engine = funded();
        resting_bid(&mut engine, 1, BOB, 1_000_000, 100_000_000);
        // Alice's ask fully fills Bob's bid. This is the operation that
        // once leaked filled makers into the book's id index (the Phase 0
        // property catch): the index entry must be GONE, or the recycle
        // below would be rejected as a live duplicate.
        filling_ask(&mut engine, 2, ALICE, 1_000_000, 100_000_000);
        assert!(
            engine.live_orders().is_empty(),
            "both orders fully consumed"
        );

        engine
            .place(gtc(1, BOB, Side::Bid, 1_000_000, 100_000_000), None)
            .unwrap();
        assert_eq!(engine.live_orders().len(), 1);
        assert_eq!(engine.ledger().locked(BOB, USD), 1_000_000);
    }

    #[test]
    fn killed_fok_id_can_be_recycled_as_a_fresh_order() {
        let mut engine = funded();
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_000_000, 5_000_000), None)
            .unwrap(); // 0.05 base resting
        // FOK for 10M lots but only 5M resting: killed, nothing traded, the
        // whole lock released — the id is dead and reusable.
        let outcome = engine
            .place(fok_bid(2, BOB, 1_000_000, 10_000_000), None)
            .unwrap();
        assert_eq!(outcome, EngineOutcome::KilledFok);

        // A smaller, now-fundable FOK under the same id is accepted as a
        // fresh order and fills: lock 50,000 ticks, settled away exactly.
        let outcome = engine
            .place(fok_bid(2, BOB, 1_000_000, 5_000_000), None)
            .unwrap();
        let EngineOutcome::Executed {
            fills,
            resting: None,
        } = outcome
        else {
            panic!("the smaller FOK now fills");
        };
        assert_eq!(fills.len(), 1);
        assert!(engine.live_orders().is_empty());
        assert_eq!(engine.ledger().free(BOB, USD), 199_999_950_000);
        assert_eq!(engine.ledger().locked(BOB, USD), 0);
    }

    #[test]
    fn cancel_of_a_canceled_id_errors_and_releases_nothing_twice() {
        let mut engine = funded();
        resting_bid(&mut engine, 1, BOB, 1_000_000, 100_000_000);
        engine.cancel(OrderId(1)).unwrap();
        let (free, locked) = (
            engine.ledger().free(BOB, USD),
            engine.ledger().locked(BOB, USD),
        );

        // The second cancel must NOT find the ghost in the book's index and
        // release the lock a second time — that would mint free funds.
        let error = engine.cancel(OrderId(1)).unwrap_err();
        assert_eq!(
            error,
            EngineError::Book(BookError::UnknownOrder { id: OrderId(1) })
        );
        assert_eq!(engine.ledger().free(BOB, USD), free);
        assert_eq!(engine.ledger().locked(BOB, USD), locked);
    }

    #[test]
    fn cancel_of_a_killed_fok_id_errors_and_releases_nothing_twice() {
        let mut engine = funded();
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_000_000, 5), None)
            .unwrap();
        let outcome = engine
            .place(fok_bid(2, BOB, 1_000_000, 10_000_000), None)
            .unwrap();
        assert_eq!(outcome, EngineOutcome::KilledFok);
        let (free, locked) = (
            engine.ledger().free(BOB, USD),
            engine.ledger().locked(BOB, USD),
        );

        // The killed FOK was never resting, so the book's index must not
        // know it — a cancel that "succeeded" here would double-release.
        let error = engine.cancel(OrderId(2)).unwrap_err();
        assert_eq!(
            error,
            EngineError::Book(BookError::UnknownOrder { id: OrderId(2) })
        );
        assert_eq!(engine.ledger().free(BOB, USD), free);
        assert_eq!(engine.ledger().locked(BOB, USD), locked);
    }

    #[test]
    fn move_of_a_killed_fok_id_errors() {
        let mut engine = funded();
        engine
            .place(gtc(1, ALICE, Side::Ask, 1_000_000, 5), None)
            .unwrap();
        let outcome = engine
            .place(fok_bid(2, BOB, 1_000_000, 10_000_000), None)
            .unwrap();
        assert_eq!(outcome, EngineOutcome::KilledFok);

        // A move targets the live map; the killed FOK left no entry — but
        // the real hazard is the opposite: an id the engine still tracks
        // while the book forgot it. Assert the engine's map is exactly the
        // maker's, then pin the move rejection.
        assert_eq!(engine.live_orders().len(), 1);
        assert!(engine.live_orders().contains_key(&OrderId(1)));
        let error = engine
            .move_order(OrderId(2), Price::from_ticks(900_000).unwrap())
            .unwrap_err();
        assert_eq!(
            error,
            EngineError::Book(BookError::UnknownOrder { id: OrderId(2) })
        );
    }

    #[test]
    fn move_of_a_canceled_id_errors() {
        let mut engine = funded();
        resting_bid(&mut engine, 1, BOB, 1_000_000, 100_000_000);
        engine.cancel(OrderId(1)).unwrap();

        let error = engine
            .move_order(OrderId(1), Price::from_ticks(1_100_000).unwrap())
            .unwrap_err();
        assert_eq!(
            error,
            EngineError::Book(BookError::UnknownOrder { id: OrderId(1) })
        );
    }

    #[test]
    fn dead_ids_leave_no_trace_in_the_live_set() {
        let mut engine = funded();
        for id in [1, 2, 3] {
            resting_bid(&mut engine, id, BOB, 1_000_000, 100_000_000);
        }
        // Price-time priority: the OLDEST bid at the level fills first, so
        // id 1 dies — not id 2. Only ids {2, 3} are live now, and the dead
        // id appears nowhere: not in the engine's map, not in the book's
        // index (the agreement assertion live_orders().len() ==
        // book().len() in the property suite fails on the very next op if
        // either structure ever leaks a dead id).
        filling_ask(&mut engine, 4, ALICE, 1_000_000, 100_000_000);
        assert_eq!(engine.live_orders().len(), 2);
        assert!(engine.live_orders().contains_key(&OrderId(2)));
        assert!(engine.live_orders().contains_key(&OrderId(3)));

        engine.cancel(OrderId(2)).unwrap();
        assert_eq!(engine.live_orders().len(), 1);
        // Cancelling the same id again is rejected — the ghost is gone.
        assert!(engine.cancel(OrderId(2)).is_err());
        // The survivor's lock is exactly its own.
        assert_eq!(
            engine.ledger().locked(BOB, USD),
            1_000_000,
            "only the surviving order's lock remains"
        );
    }

    // ---- Property tests (the Phase 1 invariants, through the engine) ------

    use proptest::prelude::*;

    /// Which order kind a randomized place submits.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Gtc,
        Ioc,
        Fok,
        Market,
    }

    /// One command in a randomized engine sequence.
    #[derive(Debug, Clone, Copy)]
    enum ECmd {
        Place {
            pick: u64,
            is_bid: bool,
            kind: Kind,
            price: u64,
            qty: u64,
        },
        Cancel {
            pick: u64,
        },
        Move {
            pick: u64,
            price: u64,
        },
        Recycle {
            pick: u64,
        },
    }

    fn ecmd_strategy() -> impl Strategy<Value = ECmd> {
        let kind = prop::sample::select(&[Kind::Gtc, Kind::Ioc, Kind::Fok, Kind::Market]);
        // Places dominate so the book actually builds up; dusty ranges
        // (price ≤ 2000 ticks, qty ≤ 10M lots) force the floor/ceil
        // rounding paths on almost every trade.
        prop_oneof![
            4 => (0u64..4, any::<bool>(), kind, 1u64..=2_000, 1u64..=10_000_000)
                .prop_map(|(pick, is_bid, kind, price, qty)| ECmd::Place {
                    pick,
                    is_bid,
                    kind,
                    price,
                    qty,
                }),
            1 => (0u64..=32).prop_map(|pick| ECmd::Cancel { pick }),
            1 => (0u64..=32, 1u64..=2_000).prop_map(|(pick, price)| ECmd::Move { pick, price }),
            1 => (0u64..=32).prop_map(|pick| ECmd::Recycle { pick }),
        ]
    }

    /// Deposits are the only money in the system: conservation means the
    /// per-currency total never moves off `users × deposit`.
    const USERS: u64 = 4;
    const QUOTE_DEPOSIT: u64 = 2_000; // ticks — bids of ≤200 ticks max, so rejections DO happen
    const BASE_DEPOSIT: u64 = 100_000_000; // lots — asks of ≤10M lots, so asks DO happen

    /// The two engine-level invariants, after every operation:
    ///
    /// 1. **Conservation** — Σ (free + locked) per currency equals deposits;
    ///    every saga (commit/release/settle) is internal reshuffling.
    /// 2. **Lock sufficiency** — every live ask locks exactly its remaining
    ///    lots; every live bid locks at least its exact remaining
    ///    obligation, floor(remaining × price / QTY_SCALE) — the integer
    ///    form of "the lock can always pay for what's left".
    ///
    /// Plus the cross-check that engine and book agree on what is live.
    fn check_engine(engine: &Engine, users: &[UserId], context: &str) {
        for (currency, deposit) in [(USD, QUOTE_DEPOSIT), (BTC, BASE_DEPOSIT)] {
            let total: u64 = users
                .iter()
                .map(|u| engine.ledger().free(*u, currency) + engine.ledger().locked(*u, currency))
                .sum::<u64>()
                + engine.ledger().fees_collected(currency);
            assert_eq!(
                total,
                users.len() as u64 * deposit,
                "conservation broken ({context}): {currency:?}"
            );
        }
        assert_eq!(
            engine.live_orders().len(),
            engine.book().len(),
            "engine/book diverged on live orders ({context})"
        );
        for (id, live) in engine.live_orders() {
            match live.side {
                Side::Ask => assert_eq!(
                    live.locked,
                    live.remaining.lot(),
                    "ask {id:?} lock must equal its remaining lots ({context})"
                ),
                Side::Bid => {
                    let price = live.price.expect("resting bids carry a price");
                    let obligation = live.remaining.lot() * price.tick() / QTY_SCALE;
                    assert!(
                        live.locked >= obligation,
                        "bid {id:?} locks {} < obligation {obligation} ({context})",
                        live.locked
                    );
                }
            }
        }
    }

    fn run_engine(cmds: &[ECmd]) -> u64 {
        run_engine_inner(cmds, FeeSchedule::zero())
    }

    /// The fee'd twin of [`run_engine`]: identical command semantics but a
    /// non-zero schedule, so the property invariants (conservation with the
    /// fee term, lock sufficiency, no-cross, determinism) are proven under
    /// real fee traffic — the self-trade-pays-both-fees path included.
    fn run_fee_engine(cmds: &[ECmd]) -> u64 {
        run_engine_inner(cmds, FeeSchedule::new(10, 25).unwrap())
    }

    fn run_engine_inner(cmds: &[ECmd], fees: FeeSchedule) -> u64 {
        let mut engine = Engine::with_fees(SymbolId(1), USD, BTC, fees);
        let users: Vec<UserId> = (1..=USERS).map(UserId).collect();
        for user in &users {
            engine
                .ledger_mut()
                .deposit(*user, USD, QUOTE_DEPOSIT)
                .unwrap();
            engine
                .ledger_mut()
                .deposit(*user, BTC, BASE_DEPOSIT)
                .unwrap();
        }
        let mut next_id: u64 = 0;
        // Live ids in placement order (stable picks, no HashMap iteration).
        let mut live_ids: Vec<OrderId> = Vec::new();

        for cmd in cmds {
            match *cmd {
                ECmd::Place {
                    pick,
                    is_bid,
                    kind,
                    price,
                    qty,
                } => {
                    let user = users[(pick % USERS) as usize];
                    let side = if is_bid { Side::Bid } else { Side::Ask };
                    next_id += 1;
                    let id = OrderId(next_id);
                    let order_type = match kind {
                        Kind::Gtc | Kind::Ioc | Kind::Fok => limit(price),
                        Kind::Market => OrderType::Market,
                    };
                    let tif = match kind {
                        Kind::Gtc => TimeInForce::Gtc,
                        Kind::Ioc | Kind::Market => TimeInForce::Ioc,
                        Kind::Fok => TimeInForce::Fok,
                    };
                    let order = Order::new(
                        id,
                        user,
                        SymbolId(1),
                        side,
                        order_type,
                        tif,
                        Qty::from_lots(qty).unwrap(),
                        next_id,
                    )
                    .unwrap();
                    // Only market bids carry a reserve (their effective
                    // limit); everywhere else it must be absent.
                    let reserve = match (kind, side) {
                        (Kind::Market, Side::Bid) => Some(Price::from_ticks(price).unwrap()),
                        _ => None,
                    };
                    // Rejections (risk gate, duplicates, overflow) are paths
                    // too — the invariants must hold after them as well.
                    let _ = engine.place(order, reserve);
                    // Track the id if it rested, so later Cancel/Move picks
                    // can target it. (Without this push, the cancel/move
                    // arms below would never fire — the suite would exercise
                    // places only. Found 2026-09-07: the list was never
                    // populated, so cancels and moves were silent no-ops.)
                    if engine.live_orders().contains_key(&id) {
                        live_ids.push(id);
                    }
                }
                ECmd::Cancel { pick } => {
                    if let Some(&id) = live_ids.get((pick as usize) % live_ids.len().max(1)) {
                        let _ = engine.cancel(id); // unknown-id rejections are fine
                    }
                }
                ECmd::Move { pick, price } => {
                    if let Some(&id) = live_ids.get((pick as usize) % live_ids.len().max(1)) {
                        // WouldCross / InsufficientForMove / OrderTooLarge
                        // rejections are fine — invariants either way.
                        let _ = engine.move_order(id, Price::from_ticks(price).unwrap());
                    }
                }
                ECmd::Recycle { pick } => {
                    // Deliberately place under a mostly-DEAD id: if the id
                    // is still live this is a compensated duplicate; if
                    // dead, it recycles as a fresh order (the sweep's
                    // finding). Conservation, lock sufficiency, and
                    // engine/book agreement must hold either way.
                    let id = (pick % 8) + 1;
                    let user = users[(pick as usize) % users.len()];
                    next_id += 1;
                    let order = Order::new(
                        OrderId(id),
                        user,
                        SymbolId(1),
                        Side::Bid,
                        limit(1_000),
                        TimeInForce::Gtc,
                        Qty::from_lots(1_000_000).unwrap(),
                        next_id,
                    )
                    .unwrap();
                    let _ = engine.place(order, None);
                }
            }
            // Drop stale picks (fully filled / dead orders left the map).
            live_ids.retain(|id| engine.live_orders().contains_key(id));
            check_engine(&engine, &users, "after op");
            // The no-cross invariant, re-proven through the engine after
            // every operation (§Test lists it; the book's own property still
            // runs — the engine composes the book unchanged).
            if let (Some(bid), Some(ask)) = (engine.book().best_bid(), engine.book().best_ask()) {
                assert!(bid < ask, "crossed book after op: {bid:?} ≥ {ask:?}");
            }
        }
        state_digest(&engine, &users)
    }

    /// A 64-bit fingerprint of the engine's full observable state: the
    /// live-order table (sorted by id — HashMap iteration order is not)
    /// plus every (user, currency) balance. Two independent runs of the
    /// same command sequence must produce equal digests; any divergence in
    /// any field breaks the equality, which is the engine-level determinism
    /// claim made testable.
    fn state_digest(engine: &Engine, users: &[UserId]) -> u64 {
        fn fold(hash: &mut u64, word: u64) {
            *hash = hash.rotate_left(27) ^ word.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        let mut hash: u64 = 0x517C_C1B7_2722_0A95;
        let mut live: Vec<(u64, LiveOrder)> = engine
            .live_orders()
            .iter()
            .map(|(id, order)| (id.0, *order))
            .collect();
        live.sort_unstable_by_key(|&(id, _)| id);
        for (id, order) in live {
            fold(&mut hash, id);
            fold(&mut hash, order.user.0);
            fold(
                &mut hash,
                match order.side {
                    Side::Bid => 0,
                    Side::Ask => 1,
                },
            );
            fold(&mut hash, order.price.map_or(0, |p| p.tick()));
            fold(&mut hash, order.remaining.lot());
            fold(&mut hash, order.currency.0);
            fold(&mut hash, order.locked);
        }
        for user in users {
            for currency in [USD, BTC] {
                fold(&mut hash, engine.ledger().free(*user, currency));
                fold(&mut hash, engine.ledger().locked(*user, currency));
            }
        }
        // The fee sink is engine-observable state too: two runs agree only
        // if the exchange's take agrees. (Zero under `run_engine` — folding
        // a constant changes the digest value, not its determinism.)
        for currency in [USD, BTC] {
            fold(&mut hash, engine.ledger().fees_collected(currency));
        }
        hash
    }

    proptest! {
        /// The Phase 1 engine invariants under randomized multi-user
        /// traffic: GTC/IOC/FOK/market-with-reserve places, cancels, moves,
        /// and dead-id recycling, with conservation and lock sufficiency
        /// asserted after EVERY operation.
        #[test]
        fn engine_conserves_and_locks_sufficiently(
            cmds in prop::collection::vec(ecmd_strategy(), 0..=80),
        ) {
            run_engine(&cmds);
        }

        /// **No crossed book, engine level** (§Test): after every operation
        /// of any sequence, best bid < best ask (or one side empty). The
        /// check runs inside [`run_engine`]; this test exists so the
        /// invariant has a named, grep-able owner in the suite.
        #[test]
        fn engine_book_never_locks_or_crosses(
            cmds in prop::collection::vec(ecmd_strategy(), 0..=80),
        ) {
            run_engine(&cmds);
        }

        /// **Determinism, engine level** (§Test: "same input → same output —
        /// there must be a test for this"): replaying the same command
        /// sequence against a fresh engine must produce an identical final
        /// state. The digest covers every live order and every balance, so
        /// any divergence anywhere breaks the equality.
        #[test]
        fn engine_replay_is_deterministic(
            cmds in prop::collection::vec(ecmd_strategy(), 0..=80),
        ) {
            let first = run_engine(&cmds);
            let second = run_engine(&cmds);
            assert_eq!(first, second, "same commands must replay identically");
        }

        /// **The fee'd twins**: the same three invariants under a non-zero
        /// schedule — conservation (extended with the fee term), lock
        /// sufficiency, no-cross, and replay determinism all hold under
        /// real fee traffic, including self-trades that pay fees on both
        /// receipts.
        #[test]
        fn fee_engine_conserves_and_locks_sufficiently(
            cmds in prop::collection::vec(ecmd_strategy(), 0..=80),
        ) {
            run_fee_engine(&cmds);
        }

        #[test]
        fn fee_engine_replay_is_deterministic(
            cmds in prop::collection::vec(ecmd_strategy(), 0..=80),
        ) {
            let first = run_fee_engine(&cmds);
            let second = run_fee_engine(&cmds);
            assert_eq!(first, second, "fee'd replay must be deterministic too");
        }
    }
}
