//! The limit order book: resting orders, the place / cancel / move
//! operation model, and the matching sweep that emits [`Fill`]s.
//!
//! Design notes (each enforced or documented by a test — see `docs/TODO.md` §5):
//!
//! - **Structure.** One [`BTreeMap<Price, PriceLevel>`] per side, each level a
//!   `VecDeque` FIFO. `BTreeMap` gives O(log P) access to the best price with
//!   zero `unsafe` and zero dependencies; the deque *is* the time priority
//!   (queue position = arrival order). This is the correctness-first Phase 0
//!   choice — TODO §10 forbids optimization; arenas/flat arrays are Phase 2.
//! - **Fills execute at the maker's price.** Price improvement belongs to the
//!   taker; a bid at 101 crossing an ask resting at 100 fills at 100.
//! - **Resting orders are always GTC.** IOC/FOK/market never rest by
//!   definition, so a resting order carries no time-in-force field.
//! - **`move` never locks or crosses the book.** Repricing into the opposite
//!   side is rejected ([`BookError::WouldCross`], Binance-amend-style) rather
//!   than silently breaking the spine invariant. Same-price moves still reset
//!   time priority (the order re-queues at the tail). Note the method is named
//!   `move_order`: `move` is a Rust keyword. Logged in the decision log.
//! - **FOK here is quantity-based** ("fill entirely or not at all", per the
//!   TODO). exchange-core's FOK-B is *budget*-based; the deviation is logged.
//! - **Known gap (deliberate):** no self-match prevention — Phase 3 work.
//!   Balance checks shipped (the ledger's commit gate); the book stays
//!   money-blind and the engine pairs every place with a ledger commit.
//!
//! No wall clock, no floats, no `unsafe` — the same ground rules as
//! [`crate::domain`], which supplies every input type used here.

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::domain::{Order, OrderId, OrderType, Price, Qty, Side, SymbolId, TimeInForce, UserId};

/// A trade that occurred during a [`OrderBook::place`] sweep.
///
/// The price is always the **maker's** resting price: a taker crossing
/// multiple levels sees one fill per level, each at that level's price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    /// The resting order that provided liquidity.
    pub maker_order_id: OrderId,
    /// Owner of the resting order.
    pub maker_user: UserId,
    /// The incoming order that took liquidity.
    pub taker_order_id: OrderId,
    /// Owner of the incoming order.
    pub taker_user: UserId,
    /// Execution price — the maker's limit, never the taker's.
    pub price: Price,
    /// Traded quantity (portion of both orders).
    pub quantity: Qty,
}

/// What [`OrderBook::place`] did with an incoming order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceOutcome {
    /// Trades executed against the opposite side, in execution order.
    pub fills: Vec<Fill>,
    /// Quantity resting on the book afterwards. `Some` **only** for GTC
    /// limit orders with an unfilled remainder — IOC, FOK and market orders
    /// never rest, and a fully-filled GTC has nothing left to rest.
    pub resting: Option<Qty>,
}

/// What [`OrderBook::reduce`] did. `reduced` is the **clamped** amount
/// actually taken off (≤ the requested `by`); `removed` is true when the
/// reduction emptied the order and it left the book entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReduceOutcome {
    /// Lots actually removed from the order's remaining quantity.
    pub reduced: Qty,
    /// What still rests (zero iff `removed`).
    pub remaining: Qty,
    /// Whether the order left the book (fully reduced).
    pub removed: bool,
}

/// A full depth snapshot of one side: levels in match order, each level's
/// queue as `(order id, owner, remaining lots)` in arrival order. Built for
/// the journal's deep digest (decision row 59) — queue order is observable
/// through it — and the honest basis for any future market-data feed.
pub type Depth = Vec<(Price, Vec<(OrderId, UserId, u64)>)>;

/// Everything the book layer can reject.
///
/// Hand-rolled like [`crate::domain::DomainError`]: the surface is small, and
/// dependencies are added one decision-log row at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    /// The order names a different symbol than this book trades.
    SymbolMismatch {
        /// Symbol this book was created for.
        expected: SymbolId,
        /// Symbol the incoming order carried.
        received: SymbolId,
    },
    /// The order id is already live on this book — engine ids must be unique
    /// or the id→locator index would silently corrupt.
    DuplicateOrder {
        /// The colliding id.
        id: OrderId,
    },
    /// Cancel/move named an id that is not resting on the book (unknown,
    /// already cancelled, or already fully filled).
    UnknownOrder {
        /// The id that was not found.
        id: OrderId,
    },
    /// A move would have locked (bid == ask) or crossed (bid > ask) the
    /// book. Rejected outright; the book is unchanged.
    WouldCross {
        /// The order that was being moved.
        id: OrderId,
        /// The best price on the opposite side it would have met.
        best_opposite: Price,
    },
    /// A restore was attempted on a book that already holds orders — state
    /// must never be piled onto state (the snapshot restore path).
    BookNotEmpty,
    /// A snapshot's captured rows describe a crossed resting book. A resting
    /// book can never be crossed (matching consumes crosses immediately), so
    /// this is corruption in the snapshot, not state.
    CrossedBook {
        /// The best captured bid.
        bid: Price,
        /// The best captured ask.
        ask: Price,
    },
    /// A snapshot row claims to be resting but carries no price — only GTC
    /// limits rest and they always carry one, so this is corruption.
    NotResting {
        /// The offending row's order id.
        id: OrderId,
    },
}

impl std::fmt::Display for BookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SymbolMismatch { expected, received } => {
                write!(
                    f,
                    "symbol mismatch: book trades {expected:?}, order is {received:?}"
                )
            }
            Self::DuplicateOrder { id } => write!(f, "order id {id:?} is already live"),
            Self::UnknownOrder { id } => write!(f, "no resting order with id {id:?}"),
            Self::WouldCross { id, best_opposite } => {
                write!(
                    f,
                    "moving order {id:?} would lock or cross the book (best opposite {best_opposite})"
                )
            }
            Self::BookNotEmpty => {
                write!(f, "cannot restore onto a book that already holds orders")
            }
            Self::CrossedBook { bid, ask } => {
                write!(f, "snapshot is crossed: best bid {bid} >= best ask {ask}")
            }
            Self::NotResting { id } => {
                write!(f, "snapshot row {id:?} is not a resting order (no price)")
            }
        }
    }
}

impl std::error::Error for BookError {}

/// An order resting on the book.
///
/// Deliberately minimal: no time-in-force (only GTC rests), no side (the
/// containing [`BookSide`] already encodes it, and cancel/move read the side
/// from the id→locator index), and no timestamp — **queue position is the
/// time priority**. The domain `Order.timestamp` sequence still exists on the
/// incoming order for later journaling; the book itself never needs it
/// because deque order already encodes arrival.
#[derive(Debug, Clone, Copy)]
struct RestingOrder {
    id: OrderId,
    user: UserId,
    price: Price,
    remaining: Qty,
}

/// All resting orders at one price, in arrival order (front = oldest).
#[derive(Debug, Default)]
struct PriceLevel {
    queue: VecDeque<RestingOrder>,
}

/// One side of the book (all bids or all asks).
#[derive(Debug)]
struct BookSide {
    /// `true` for bids (best = highest price), `false` for asks (best =
    /// lowest). One flag lets every "best-first" traversal share one code path.
    best_is_highest: bool,
    levels: BTreeMap<Price, PriceLevel>,
}

impl BookSide {
    fn new(best_is_highest: bool) -> Self {
        Self {
            best_is_highest,
            levels: BTreeMap::new(),
        }
    }

    /// Levels in *match order* — best price first, because a sweep always
    /// consumes the best level before looking at the next one.
    fn levels_in_match_order(&self) -> Box<dyn Iterator<Item = (&Price, &PriceLevel)> + '_> {
        // Boxed because the two branches have different iterator types. The
        // allocation is fine here — sweeps walk many levels and amortize the
        // box. (`best()` was split out of this in Phase 2 optimization #1;
        // the profiler share turned out to be blur — see `best`'s doc.)
        if self.best_is_highest {
            Box::new(self.levels.iter().rev())
        } else {
            Box::new(self.levels.iter())
        }
    }

    /// The best price on this side, if any orders rest here.
    ///
    /// Direct `first`/`last` key access — O(log n), zero allocation. Levels
    /// never hold empty queues (`rest`/`remove_order` maintain that), so the
    /// first key always has orders behind it.
    ///
    /// Provenance (Phase 2 optimization #1): the profiler attributed ~4% of
    /// on-CPU samples to the old boxed-iterator form, but the 1M-op harness
    /// showed NO end-to-end change (p50 250→250 ns) — the share was
    /// attribution blur (allocator reuse makes a hot same-size-class box
    /// nearly free). Kept as a strict simplification of the hot gate, with
    /// no performance claim.
    fn best(&self) -> Option<Price> {
        if self.best_is_highest {
            self.levels.keys().next_back().copied()
        } else {
            self.levels.keys().next().copied()
        }
    }

    /// Append an order to the tail of its price level (= newest time priority).
    fn rest(&mut self, order: RestingOrder) {
        self.levels
            .entry(order.price)
            .or_default()
            .queue
            .push_back(order);
    }

    /// Remove a specific live order from the level at `price`.
    ///
    /// Scans the level's queue for the id — O(queue length). A locator index
    /// ([`OrderBook::index`]) narrows the search to one level; the linear scan
    /// within it is the Phase 0-honest cost, and mid-queue removal needs no
    /// tombstones because we locate by id every time.
    fn remove_order(&mut self, price: Price, id: OrderId) -> Option<RestingOrder> {
        let level = self.levels.get_mut(&price)?;
        let position = level.queue.iter().position(|order| order.id == id)?;
        let order = level.queue.remove(position)?;
        let now_empty = level.queue.is_empty();
        // Empty levels are removed so `best()` never sees a ghost price.
        if now_empty {
            self.levels.remove(&price);
        }
        Some(order)
    }

    /// Reduce a resting order's remaining quantity by `by`, clamped to what
    /// remains (the clamp lives HERE so the invariant cannot be bypassed by
    /// a future caller). Returns `(actually_reduced, new_remaining)`.
    ///
    /// Queue position is untouched — a reduce is not a reprice, and time
    /// priority survives it. A fully-reduced order leaves the book exactly
    /// as [`BookSide::remove_order`] would (mid-queue removal + empty-level
    /// teardown), keeping the two paths in sync by construction.
    fn reduce_order(&mut self, price: Price, id: OrderId, by: Qty) -> Option<(Qty, Qty)> {
        let level = self.levels.get_mut(&price)?;
        let position = level.queue.iter().position(|order| order.id == id)?;
        let clamped_lots = by.lot().min(level.queue[position].remaining.lot());
        let clamped = Qty::from_lots(clamped_lots)
            .expect("clamped lots > 0: resting orders always hold a positive remainder");
        let order = &mut level.queue[position];
        let new_remaining = order
            .remaining
            .checked_sub(clamped)
            .expect("clamped ≤ remaining by the min above");
        order.remaining = new_remaining;
        if new_remaining.lot() == 0 {
            level.queue.remove(position).expect("position just checked");
            if level.queue.is_empty() {
                self.levels.remove(&price);
            }
        }
        Some((clamped, new_remaining))
    }

    /// Total available quantity at prices a taker with `bound` would accept,
    /// in match order (used by the FOK pre-check).
    fn available_lots(&self, taker_side: Side, bound: Option<Price>) -> u64 {
        let mut total: u64 = 0;
        for (price, level) in self.levels_in_match_order() {
            // Match order is monotonic in acceptability: once one level is
            // unacceptable, every later one is too — stop early.
            if !price_acceptable(taker_side, bound, *price) {
                break;
            }
            for order in &level.queue {
                total = total
                    .checked_add(order.remaining.lot())
                    .expect("resting quantity sums stay far below u64::MAX");
            }
        }
        total
    }

    /// Sum of every resting quantity on this side (query for tests/market data).
    fn total_lots(&self) -> u64 {
        self.levels
            .values()
            .flat_map(|level| level.queue.iter())
            .map(|order| order.remaining.lot())
            .fold(0, |acc, lots| {
                acc.checked_add(lots)
                    .expect("resting quantity sums stay far below u64::MAX")
            })
    }

    /// Number of live resting orders on this side.
    fn len(&self) -> usize {
        self.levels.values().map(|level| level.queue.len()).sum()
    }

    /// Full depth in match order with per-level queue order (see
    /// [`OrderBook::depth`]): levels iterate in the side's sort order, each
    /// order as `(id, user, remaining lots)`. Owned `Vec`s — a snapshot, not
    /// a borrow, so callers (the journal digest) can fold without fighting
    /// the book's internals.
    fn depth(&self) -> Depth {
        self.levels_in_match_order()
            .map(|(price, level)| {
                (
                    *price,
                    level
                        .queue
                        .iter()
                        .map(|order| (order.id, order.user, order.remaining.lot()))
                        .collect(),
                )
            })
            .collect()
    }

    /// Consume the opposite side best-first while the taker still wants more
    /// and prices remain acceptable, recording one [`Fill`] per maker trade.
    ///
    /// `bound` is the taker's limit price (`None` for market orders). The
    /// maker keeps its queue slot on a partial fill — its earlier arrival
    /// time is untouched, which is exactly the price-time rule.
    fn sweep_against(
        &mut self,
        taker_side: Side,
        bound: Option<Price>,
        taker: &mut Taker,
        fills: &mut Vec<Fill>,
        index: &mut HashMap<OrderId, Locator>,
    ) {
        while taker.remaining.lot() > 0 {
            let Some(best) = self.best() else { break };
            if !price_acceptable(taker_side, bound, best) {
                break;
            }
            let Some(level) = self.levels.get_mut(&best) else {
                break;
            };

            while taker.remaining.lot() > 0 {
                let Some(mut maker) = level.queue.pop_front() else {
                    break;
                };
                let traded = if maker.remaining <= taker.remaining {
                    maker.remaining
                } else {
                    taker.remaining
                };
                fills.push(Fill {
                    maker_order_id: maker.id,
                    maker_user: maker.user,
                    taker_order_id: taker.id,
                    taker_user: taker.user,
                    price: best,
                    quantity: traded,
                });
                // `traded` is the min of the two remainders, so neither
                // subtraction can underflow — the expects document the
                // invariant rather than guard against a real case.
                maker.remaining = maker
                    .remaining
                    .checked_sub(traded)
                    .expect("traded ≤ maker remaining");
                taker.remaining = taker
                    .remaining
                    .checked_sub(traded)
                    .expect("traded ≤ taker remaining");
                if maker.remaining.lot() > 0 {
                    // Taker exhausted first: the partially-filled maker keeps
                    // the front of the queue.
                    level.queue.push_front(maker);
                    break;
                }
                // Fully filled: the maker is gone from the queue — it must
                // leave the id index too, or a later cancel/move of its id
                // would find a locator pointing at nothing (bug caught by
                // the property suite; regression-tested below).
                index.remove(&maker.id);
            }

            if level.queue.is_empty() {
                self.levels.remove(&best);
            }
        }
    }
}

/// Can a taker on `taker_side` with price bound `bound` trade at `maker_price`?
///
/// `None` bound = market order = any price. A limit bid accepts asks at or
/// *below* its limit; a limit ask accepts bids at or *above* its limit —
/// equality crosses (matching at the limit is not a lock: the taker executes
/// and never rests).
fn price_acceptable(taker_side: Side, bound: Option<Price>, maker_price: Price) -> bool {
    match bound {
        None => true,
        Some(limit) => match taker_side {
            Side::Bid => maker_price <= limit,
            Side::Ask => maker_price >= limit,
        },
    }
}

/// The incoming order, mid-execution: who it belongs to and what's left
/// to trade. Bundled so the sweep's signature stays readable instead of
/// threading id/user/remaining as three parallel arguments.
#[derive(Debug, Clone, Copy)]
struct Taker {
    id: OrderId,
    user: UserId,
    remaining: Qty,
}

/// Where a live order sits: which side, at which price level. The id → locator
/// index makes cancel/move O(log n) instead of a whole-book scan.
#[derive(Debug, Clone, Copy)]
struct Locator {
    side: Side,
    price: Price,
}

/// The order book for one symbol.
///
/// Invariants (all checked by tests, two of them property-based):
/// 1. best bid < best ask whenever both sides are non-empty (never locked,
///    never crossed);
/// 2. queue order within a level == arrival order (price-time priority);
/// 3. quantity is conserved: filled + cancelled + resting == placed.
#[derive(Debug)]
pub struct OrderBook {
    symbol: SymbolId,
    bids: BookSide,
    asks: BookSide,
    index: HashMap<OrderId, Locator>,
}

impl OrderBook {
    /// Create an empty book for one symbol (one book per instrument).
    #[must_use]
    pub fn new(symbol: SymbolId) -> Self {
        Self {
            symbol,
            // Bids sort best=highest, asks best=lowest — the flag drives all
            // best-first traversals in one place.
            bids: BookSide::new(true),
            asks: BookSide::new(false),
            index: HashMap::new(),
        }
    }

    /// The symbol this book trades.
    #[must_use]
    pub fn symbol(&self) -> SymbolId {
        self.symbol
    }

    /// Highest resting bid, if any.
    #[must_use]
    pub fn best_bid(&self) -> Option<Price> {
        self.bids.best()
    }

    /// Lowest resting ask, if any.
    #[must_use]
    pub fn best_ask(&self) -> Option<Price> {
        self.asks.best()
    }

    /// Number of live resting orders on both sides.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bids.len() + self.asks.len()
    }

    /// True when no orders rest on either side.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Sum of all resting quantity, in raw lots (engine-math accessor, same
    /// spirit as [`Price::tick`] / [`Qty::lot`]).
    #[must_use]
    pub fn total_resting_lots(&self) -> u64 {
        self.bids.total_lots() + self.asks.total_lots()
    }

    /// Sum of resting quantity on one side, in raw lots (per-side depth for
    /// market-data queries and the conservation property test).
    #[must_use]
    pub fn resting_lots(&self, side: Side) -> u64 {
        self.side_ref(side).total_lots()
    }

    /// Full depth of one side: every price level in **match order** (best
    /// first), each with its queue in **arrival order** — the complete
    /// price-time structure, not an aggregate.
    ///
    /// Built for the journal's replay digest (decision row 59): an aggregate
    /// like [`OrderBook::resting_lots`] cannot see queue order, so a replay
    /// that subtly broke price-time priority would still digest equal. This
    /// accessor makes priority observable. Also the honest basis for any
    /// future market-data depth feed.
    #[must_use]
    pub fn depth(&self, side: Side) -> Depth {
        self.side_ref(side).depth()
    }

    /// Submit a new order: sweep the opposite side, then rest any GTC
    /// remainder. The returned [`PlaceOutcome`] carries every fill and what
    /// (if anything) now rests.
    ///
    /// Ids must be unique **among live orders only**: an id whose order has
    /// died (fully filled, canceled, or killed) left no trace and may be
    /// reused as a fresh, independent order — venue-standard id recycling
    /// (edge-case sweep, 2026-09-14). A duplicate of an id still on the
    /// book is rejected, or the id→locator index would silently corrupt.
    ///
    /// # Errors
    /// - [`BookError::SymbolMismatch`] — order is for another symbol;
    /// - [`BookError::DuplicateOrder`] — id already live on this book.
    pub fn place(&mut self, order: Order) -> Result<PlaceOutcome, BookError> {
        if order.symbol != self.symbol {
            return Err(BookError::SymbolMismatch {
                expected: self.symbol,
                received: order.symbol,
            });
        }
        if self.index.contains_key(&order.id) {
            return Err(BookError::DuplicateOrder { id: order.id });
        }

        // Matching bound: a limit order crosses only up to its limit; a market
        // order takes any price (and is IOC-only by domain construction, so it
        // can never reach the resting branch below).
        let bound = match order.order_type {
            OrderType::Limit { price } => Some(price),
            OrderType::Market => None,
        };

        // FOK measures *before* touching anything: all-or-nothing means we
        // must never partially execute and then discover the shortfall.
        if order.tif == TimeInForce::Fok {
            let available = self
                .side_ref(order.side.opposite())
                .available_lots(order.side, bound);
            if available < order.quantity.lot() {
                return Ok(PlaceOutcome {
                    fills: Vec::new(),
                    resting: None,
                });
            }
        }

        let mut taker = Taker {
            id: order.id,
            user: order.user,
            remaining: order.quantity,
        };
        let mut fills = Vec::new();

        // Two &mut into different fields at once is fine — the sides are
        // disjoint. The taker sweeps the opposite side; any remainder rests
        // on its own side.
        let (own, opposite): (&mut BookSide, &mut BookSide) = match order.side {
            Side::Bid => (&mut self.bids, &mut self.asks),
            Side::Ask => (&mut self.asks, &mut self.bids),
        };
        opposite.sweep_against(order.side, bound, &mut taker, &mut fills, &mut self.index);

        let mut resting = None;
        if taker.remaining.lot() > 0 && order.tif == TimeInForce::Gtc {
            let price = bound.expect("GTC orders are limit orders and always carry a price");
            own.rest(RestingOrder {
                id: order.id,
                user: order.user,
                price,
                remaining: taker.remaining,
            });
            self.index.insert(
                order.id,
                Locator {
                    side: order.side,
                    price,
                },
            );
            resting = Some(taker.remaining);
        }

        Ok(PlaceOutcome { fills, resting })
    }

    /// Remove a resting order from the book.
    ///
    /// # Errors
    /// [`BookError::UnknownOrder`] if the id is not live on this book.
    pub fn cancel(&mut self, id: OrderId) -> Result<Qty, BookError> {
        let locator = self
            .index
            .remove(&id)
            .ok_or(BookError::UnknownOrder { id })?;
        let book_side = match locator.side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        let removed = book_side
            .remove_order(locator.price, id)
            .expect("index and book are in sync: every index entry has a resting order");
        Ok(removed.remaining)
    }

    /// Reduce a resting order's size by `by` lots — the exchange-core
    /// `reduceOrder` (adopt-deferred from the Phase 2 re-read, landed in
    /// Phase 3 where the reduce event and the money leg belong).
    ///
    /// Semantics per the adopt row: `by` is **clamped to the remaining
    /// quantity** (so `by ≥ remaining` is an exact removal), a partial
    /// reduce **keeps queue position** (a reduce is not a reprice — time
    /// priority is untouched), and a full reduction removes the order via
    /// the same path a cancel would (index entry dropped; emptied level
    /// torn down).
    ///
    /// No money moves here — the book knows quantities, not locks. The
    /// engine pairs this with the matching partial lock release.
    ///
    /// # Errors
    /// [`BookError::UnknownOrder`] if the id is not live on this book.
    pub fn reduce(&mut self, id: OrderId, by: Qty) -> Result<ReduceOutcome, BookError> {
        let locator = *self.index.get(&id).ok_or(BookError::UnknownOrder { id })?;
        let book_side = match locator.side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        let (reduced, remaining) = book_side
            .reduce_order(locator.price, id, by)
            .expect("index and book are in sync: every index entry has a resting order");
        let removed = remaining.lot() == 0;
        if removed {
            self.index.remove(&id);
        }
        Ok(ReduceOutcome {
            reduced,
            remaining,
            removed,
        })
    }

    /// Restore resting orders from a snapshot (the journal's snapshot slice,
    /// decision row 61): rebuild both sides exactly as captured — per level,
    /// queue order is the snapshot's queue order, which *is* arrival order,
    /// so price-time priority is preserved by construction, not re-derived.
    ///
    /// Must be called on an **empty** book, with each side's rows in
    /// best-first (match) order. Rows are resting orders by definition — only
    /// GTC limits ever rest, and the engine's live map holds only resting
    /// orders — so no TIF check is needed or possible here; the engine's
    /// restore path documents that invariant at its capture site.
    /// All-or-nothing: a rejected restore leaves the book untouched.
    ///
    /// # Errors
    /// - [`BookError::BookNotEmpty`] — the book already holds orders;
    /// - [`BookError::DuplicateOrder`] — the same id appears twice;
    /// - [`BookError::CrossedBook`] — a captured bid ≥ a captured ask (a
    ///   resting book can never be crossed — this is corruption, not state).
    pub fn restore_depth(
        &mut self,
        bids: &[(OrderId, UserId, Price, Qty)],
        asks: &[(OrderId, UserId, Price, Qty)],
    ) -> Result<(), BookError> {
        // Refuse to pile state onto state. Checked before any mutation so the
        // all-or-nothing guarantee holds without any rollback machinery.
        if !self.is_empty() {
            return Err(BookError::BookNotEmpty);
        }
        // Validate first, mutate second: every error below must leave the
        // book exactly as it was.
        let mut seen = std::collections::HashSet::new();
        for (id, _, _, _) in bids.iter().chain(asks.iter()) {
            if !seen.insert(*id) {
                return Err(BookError::DuplicateOrder { id: *id });
            }
        }
        // Order-independent no-cross: the HIGHEST bid must sit below the
        // LOWEST ask, wherever they sit in the input rows — a hand-built
        // snapshot need not arrive in match order (capture does, and the
        // engine validates the same rule before calling; this makes the
        // public method safe standalone).
        let best_bid = bids.iter().map(|row| row.2).max();
        let best_ask = asks.iter().map(|row| row.2).min();
        if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
            if bid >= ask {
                return Err(BookError::CrossedBook { bid, ask });
            }
        }
        // The captured rows are already validated as GTC limits by the
        // caller's contract; the book rebuilds its own internal structures
        // from the same data it would have built them from at place time.
        for (rows, side) in [(bids, Side::Bid), (asks, Side::Ask)] {
            let book_side = match side {
                Side::Bid => &mut self.bids,
                Side::Ask => &mut self.asks,
            };
            // Group rows by price preserving first-seen order per level.
            for (id, user, price, remaining) in rows {
                book_side
                    .levels
                    .entry(*price)
                    .or_default()
                    .queue
                    .push_back(RestingOrder {
                        id: *id,
                        user: *user,
                        price: *price,
                        remaining: *remaining,
                    });
                self.index.insert(
                    *id,
                    Locator {
                        side,
                        price: *price,
                    },
                );
            }
        }
        Ok(())
    }

    /// Reprice a resting order (the exchange-core `move` operation).
    ///
    /// Re-queueing at the tail of the (new) level is what "resets time
    /// priority" means here — queue position *is* priority. A same-price move
    /// also moves to the back. Remaining quantity and identity are preserved.
    ///
    /// # Errors
    /// - [`BookError::UnknownOrder`] — id not live;
    /// - [`BookError::WouldCross`] — the new price would lock or cross the
    ///   book (rejected, book unchanged).
    pub fn move_order(&mut self, id: OrderId, new_price: Price) -> Result<(), BookError> {
        let locator = *self.index.get(&id).ok_or(BookError::UnknownOrder { id })?;

        // A resting order that repriced into the opposite side would create a
        // locked/crossed book — the one operation that could, since place
        // sweeps and cancel only removes. Reject before touching anything.
        if let Some(best_opposite) = self.side_ref(locator.side.opposite()).best() {
            let would_cross = match locator.side {
                Side::Bid => new_price >= best_opposite,
                Side::Ask => new_price <= best_opposite,
            };
            if would_cross {
                return Err(BookError::WouldCross { id, best_opposite });
            }
        }

        let book_side = match locator.side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        let mut order = book_side
            .remove_order(locator.price, id)
            .expect("index and book are in sync: every index entry has a resting order");
        order.price = new_price;
        book_side.rest(order);
        self.index.insert(
            id,
            Locator {
                side: locator.side,
                price: new_price,
            },
        );
        Ok(())
    }

    /// Immutable access to one side (FOK pre-check, cross-checks).
    fn side_ref(&self, side: Side) -> &BookSide {
        match side {
            Side::Bid => &self.bids,
            Side::Ask => &self.asks,
        }
    }
}

#[cfg(test)]
mod tests;
