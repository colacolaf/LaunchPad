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
//! - **Known Phase 0 gaps (deliberate):** no self-match prevention and no
//!   balance checks — both are Phase 3 risk-control work.
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
        // Boxed because the two branches have different iterator types; this
        // is a query path, not the hot path (Phase 2 revisits).
        if self.best_is_highest {
            Box::new(self.levels.iter().rev())
        } else {
            Box::new(self.levels.iter())
        }
    }

    /// The best price on this side, if any orders rest here.
    fn best(&self) -> Option<Price> {
        self.levels_in_match_order().next().map(|(price, _)| *price)
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

    /// Submit a new order: sweep the opposite side, then rest any GTC
    /// remainder. The returned [`PlaceOutcome`] carries every fill and what
    /// (if anything) now rests.
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
mod tests {
    use super::*;

    // ---- Builders (tests may unwrap; shipped code may not) --------------

    fn limit(price_ticks: u64) -> OrderType {
        OrderType::Limit {
            price: Price::from_ticks(price_ticks).unwrap(),
        }
    }

    /// GTC limit order with id==timestamp, the usual test shape.
    fn gtc(id: u64, side: Side, price_ticks: u64, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            UserId(1),
            SymbolId(1),
            side,
            limit(price_ticks),
            TimeInForce::Gtc,
            Qty::from_lots(lots).unwrap(),
            id, // engine-assigned sequence; uniqueness is what matters here
        )
        .unwrap()
    }

    fn ioc(id: u64, side: Side, price_ticks: u64, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            UserId(2),
            SymbolId(1),
            side,
            limit(price_ticks),
            TimeInForce::Ioc,
            Qty::from_lots(lots).unwrap(),
            id,
        )
        .unwrap()
    }

    fn fok_bid(id: u64, price_ticks: u64, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            UserId(2),
            SymbolId(1),
            Side::Bid,
            limit(price_ticks),
            TimeInForce::Fok,
            Qty::from_lots(lots).unwrap(),
            id,
        )
        .unwrap()
    }

    fn market_bid(id: u64, lots: u64) -> Order {
        Order::new(
            OrderId(id),
            UserId(2),
            SymbolId(1),
            Side::Bid,
            OrderType::Market,
            TimeInForce::Ioc, // market is IOC-only by domain rule
            Qty::from_lots(lots).unwrap(),
            id,
        )
        .unwrap()
    }

    // ---- Empty book & resting -------------------------------------------

    #[test]
    fn empty_book_has_no_best_prices() {
        let book = OrderBook::new(SymbolId(1));
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
        assert!(book.is_empty());
        assert_eq!(book.len(), 0);
        assert_eq!(book.total_resting_lots(), 0);
    }

    #[test]
    fn gtc_limit_rests_and_updates_best() {
        let mut book = OrderBook::new(SymbolId(1));
        let outcome = book.place(gtc(1, Side::Bid, 1_005_000, 2)).unwrap(); // 100.5
        assert_eq!(outcome.fills, Vec::new());
        assert_eq!(outcome.resting, Some(Qty::from_lots(2).unwrap()));
        assert_eq!(book.best_bid(), Some(Price::from_ticks(1_005_000).unwrap()));
        assert_eq!(book.best_ask(), None);
        assert_eq!(book.len(), 1);
    }

    #[test]
    fn levels_sort_best_first_on_both_sides() {
        let mut book = OrderBook::new(SymbolId(1));
        for (id, price) in [(1, 990_000), (2, 1_010_000), (3, 1_000_000)] {
            book.place(gtc(id, Side::Bid, price, 1)).unwrap();
        }
        for (id, price) in [(4, 1_040_000), (5, 1_020_000), (6, 1_030_000)] {
            book.place(gtc(id, Side::Ask, price, 1)).unwrap();
        }
        // Best bid = highest (1.01), best ask = lowest (1.02).
        assert_eq!(book.best_bid(), Some(Price::from_ticks(1_010_000).unwrap()));
        assert_eq!(book.best_ask(), Some(Price::from_ticks(1_020_000).unwrap()));
        assert_eq!(book.len(), 6);
    }

    // ---- Matching: limit crossing ----------------------------------------

    #[test]
    fn crossing_limit_fills_at_maker_price_and_remainder_rests() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 1_000_000, 5)).unwrap(); // ask 5 @ 100
        let outcome = book.place(gtc(2, Side::Bid, 1_010_000, 10)).unwrap(); // bid 10 @ 101

        assert_eq!(outcome.fills.len(), 1);
        let fill = &outcome.fills[0];
        assert_eq!(fill.maker_order_id, OrderId(1));
        assert_eq!(fill.taker_order_id, OrderId(2));
        assert_eq!(fill.price, Price::from_ticks(1_000_000).unwrap()); // maker's price
        assert_eq!(fill.quantity, Qty::from_lots(5).unwrap());
        assert_eq!(outcome.resting, Some(Qty::from_lots(5).unwrap()));

        assert_eq!(book.best_bid(), Some(Price::from_ticks(1_010_000).unwrap()));
        assert_eq!(book.best_ask(), None); // level consumed
        assert_eq!(book.total_resting_lots(), 5);
    }

    #[test]
    fn taker_gets_price_improvement_across_levels() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 995_000, 3)).unwrap(); // 99.5
        book.place(gtc(2, Side::Ask, 1_000_000, 2)).unwrap(); // 100.0

        let outcome = book.place(gtc(3, Side::Bid, 1_000_000, 4)).unwrap(); // bid 4 @ 100
        let prices: Vec<u64> = outcome.fills.iter().map(|f| f.price.tick()).collect();
        assert_eq!(prices, vec![995_000, 1_000_000]); // best level first
        let quantities: Vec<u64> = outcome.fills.iter().map(|f| f.quantity.lot()).collect();
        assert_eq!(quantities, vec![3, 1]);
        assert_eq!(outcome.resting, None); // fully filled
        assert_eq!(book.best_ask(), Some(Price::from_ticks(1_000_000).unwrap()));
    }

    #[test]
    fn fully_matched_order_leaves_no_trace() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 1_000_000, 5)).unwrap();
        let outcome = book.place(gtc(2, Side::Bid, 1_010_000, 5)).unwrap();
        assert_eq!(outcome.resting, None);
        assert!(book.is_empty());
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
    }

    // ---- Matching: execution policies ------------------------------------

    #[test]
    fn ioc_fills_what_it_can_and_never_rests() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 1_000_000, 5)).unwrap();
        let outcome = book.place(ioc(2, Side::Bid, 1_005_000, 10)).unwrap();
        assert_eq!(outcome.fills.len(), 1);
        assert_eq!(outcome.fills[0].quantity, Qty::from_lots(5).unwrap());
        assert_eq!(outcome.resting, None, "IOC remainder must die, never rest");
        assert!(book.is_empty());
    }

    #[test]
    fn ioc_with_no_crossing_executes_nothing() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 1_000_000, 5)).unwrap();
        let outcome = book.place(ioc(2, Side::Bid, 999_999, 10)).unwrap(); // below the ask
        assert!(outcome.fills.is_empty());
        assert_eq!(outcome.resting, None);
        assert_eq!(book.best_ask(), Some(Price::from_ticks(1_000_000).unwrap()));
    }

    #[test]
    fn fok_executes_all_or_nothing() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 1_000_000, 5)).unwrap();

        // Only 5 available, 10 wanted: kill, untouched book.
        let killed = book.place(fok_bid(2, 1_010_000, 10)).unwrap();
        assert!(killed.fills.is_empty());
        assert_eq!(killed.resting, None);
        assert_eq!(book.best_ask(), Some(Price::from_ticks(1_000_000).unwrap()));
        assert_eq!(book.len(), 1);

        // Now 10 available across two levels: all-or-nothing succeeds.
        book.place(gtc(3, Side::Ask, 1_000_000, 5)).unwrap();
        let filled = book.place(fok_bid(4, 1_010_000, 10)).unwrap();
        assert_eq!(filled.fills.len(), 2);
        assert_eq!(filled.resting, None);
        assert!(book.is_empty());
    }

    #[test]
    fn market_order_sweeps_levels_and_never_rests() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 1_000_000, 3)).unwrap();
        book.place(gtc(2, Side::Ask, 1_010_000, 4)).unwrap();

        let outcome = book.place(market_bid(3, 10)).unwrap(); // wants 10, only 7 exist
        let prices: Vec<u64> = outcome.fills.iter().map(|f| f.price.tick()).collect();
        assert_eq!(prices, vec![1_000_000, 1_010_000]);
        let quantities: Vec<u64> = outcome.fills.iter().map(|f| f.quantity.lot()).collect();
        assert_eq!(quantities, vec![3, 4]);
        assert_eq!(outcome.resting, None, "market remainder dies, never rests");
        assert!(book.is_empty());
    }

    #[test]
    fn market_order_into_empty_book_executes_nothing() {
        let mut book = OrderBook::new(SymbolId(1));
        let outcome = book.place(market_bid(1, 5)).unwrap();
        assert!(outcome.fills.is_empty());
        assert_eq!(outcome.resting, None);
    }

    // ---- Cancel ------------------------------------------------------------

    #[test]
    fn cancel_of_fully_filled_order_is_unknown_not_panic() {
        // Regression test (found by the property suite): a sweep that fully
        // fills a resting maker must evict it from the id index, or a later
        // cancel of that id desyncs index and book.
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 1_000_000, 5)).unwrap();
        book.place(gtc(2, Side::Bid, 1_010_000, 5)).unwrap(); // fully fills maker 1
        assert_eq!(
            book.cancel(OrderId(1)),
            Err(BookError::UnknownOrder { id: OrderId(1) })
        );
        // Same for move: the id is gone, not stale.
        assert!(matches!(
            book.move_order(OrderId(1), Price::from_ticks(1_020_000).unwrap()),
            Err(BookError::UnknownOrder { .. })
        ));
    }

    #[test]
    fn cancel_of_partially_filled_maker_still_works() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Ask, 1_000_000, 5)).unwrap();
        book.place(gtc(2, Side::Bid, 1_010_000, 2)).unwrap(); // partial: maker keeps 3
        // The maker is still live: cancel must succeed and return the rest.
        assert_eq!(book.cancel(OrderId(1)), Ok(Qty::from_lots(3).unwrap()));
        assert!(book.is_empty());
    }

    #[test]
    fn cancel_removes_order_and_updates_best() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Bid, 1_000_000, 2)).unwrap();
        book.place(gtc(2, Side::Bid, 1_010_000, 2)).unwrap();

        let cancelled = book.cancel(OrderId(2)).unwrap();
        assert_eq!(cancelled, Qty::from_lots(2).unwrap());
        assert_eq!(book.best_bid(), Some(Price::from_ticks(1_000_000).unwrap()));

        book.cancel(OrderId(1)).unwrap();
        assert!(book.is_empty());
        assert_eq!(book.best_bid(), None);

        // Unknown ids (never placed, or already cancelled/filled) error.
        assert_eq!(
            book.cancel(OrderId(1)),
            Err(BookError::UnknownOrder { id: OrderId(1) })
        );
    }

    #[test]
    fn cancel_middle_of_queue_preserves_others_priority() {
        let mut book = OrderBook::new(SymbolId(1));
        for id in [1, 2, 3] {
            book.place(gtc(id, Side::Bid, 1_000_000, 2)).unwrap();
        }
        book.cancel(OrderId(2)).unwrap();

        // Sweep 5 of the 4 resting lots: fills must come out A then C.
        let outcome = book.place(ioc(9, Side::Ask, 999_999, 5)).unwrap();
        let makers: Vec<u64> = outcome.fills.iter().map(|f| f.maker_order_id.0).collect();
        assert_eq!(makers, vec![1, 3]);
    }

    // ---- Move ----------------------------------------------------------------

    #[test]
    fn move_changes_price_and_resets_time_priority() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Bid, 1_000_000, 2)).unwrap(); // A
        book.place(gtc(2, Side::Bid, 1_000_000, 2)).unwrap(); // B (behind A)

        // A steps away to 0.99, then back to 1.00 — it must re-queue *behind* B.
        book.move_order(OrderId(1), Price::from_ticks(990_000).unwrap())
            .unwrap();
        assert_eq!(book.best_bid(), Some(Price::from_ticks(1_000_000).unwrap())); // B is best now
        book.move_order(OrderId(1), Price::from_ticks(1_000_000).unwrap())
            .unwrap();

        let outcome = book.place(ioc(9, Side::Ask, 999_999, 3)).unwrap();
        let makers: Vec<u64> = outcome.fills.iter().map(|f| f.maker_order_id.0).collect();
        assert_eq!(makers, vec![2, 1], "B keeps time priority after A's move");
    }

    #[test]
    fn move_rejects_locked_or_crossed_book() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Bid, 990_000, 2)).unwrap(); // bid 0.99
        book.place(gtc(2, Side::Ask, 1_000_000, 2)).unwrap(); // ask 1.00

        // Bid at the ask would lock; above it would cross. Both rejected.
        assert_eq!(
            book.move_order(OrderId(1), Price::from_ticks(1_000_000).unwrap()),
            Err(BookError::WouldCross {
                id: OrderId(1),
                best_opposite: Price::from_ticks(1_000_000).unwrap()
            })
        );
        assert!(matches!(
            book.move_order(OrderId(1), Price::from_ticks(1_010_000).unwrap()),
            Err(BookError::WouldCross { .. })
        ));
        // Ask repricing down into the bid is rejected too.
        assert!(matches!(
            book.move_order(OrderId(2), Price::from_ticks(990_000).unwrap()),
            Err(BookError::WouldCross { .. })
        ));

        // The book is unchanged by every rejected move.
        assert_eq!(book.best_bid(), Some(Price::from_ticks(990_000).unwrap()));
        assert_eq!(book.best_ask(), Some(Price::from_ticks(1_000_000).unwrap()));
    }

    #[test]
    fn move_of_unknown_order_errors() {
        let mut book = OrderBook::new(SymbolId(1));
        assert_eq!(
            book.move_order(OrderId(7), Price::from_ticks(1).unwrap()),
            Err(BookError::UnknownOrder { id: OrderId(7) })
        );
    }

    // ---- Place validation ------------------------------------------------------

    #[test]
    fn place_rejects_duplicate_live_id() {
        let mut book = OrderBook::new(SymbolId(1));
        book.place(gtc(1, Side::Bid, 1_000_000, 1)).unwrap();
        assert_eq!(
            book.place(gtc(1, Side::Bid, 1_000_000, 1)),
            Err(BookError::DuplicateOrder { id: OrderId(1) })
        );
    }

    #[test]
    fn place_rejects_wrong_symbol() {
        let mut book = OrderBook::new(SymbolId(1));
        let wrong = Order::new(
            OrderId(1),
            UserId(1),
            SymbolId(2), // different book
            Side::Bid,
            limit(1_000_000),
            TimeInForce::Gtc,
            Qty::from_lots(1).unwrap(),
            1,
        )
        .unwrap();
        assert!(matches!(
            book.place(wrong),
            Err(BookError::SymbolMismatch {
                expected: SymbolId(1),
                received: SymbolId(2)
            })
        ));
    }

    // The `OrderAction` vocabulary (Place/Move/Cancel) is what the engine
    // layer will dispatch on; the book's methods mirror it one-to-one.
    // (No static check is possible — the mirroring is structural, verified
    // by the operation tests above, not by a type-level assertion.)

    // ---- Property tests (docs/TODO.md §6, the four invariants) -----------

    use proptest::prelude::*;

    /// One command in a randomized operation sequence.
    #[derive(Debug, Clone, Copy)]
    enum Cmd {
        Place { is_bid: bool, price: u64, qty: u64 },
        Cancel { pick: u64 },
        Move { pick: u64, price: u64 },
    }

    fn cmd_strategy() -> impl Strategy<Value = Cmd> {
        prop_oneof![
            // Weight places heavily: the book must actually build up.
            3 => (any::<bool>(), 1u64..=200, 1u64..=1_000)
                .prop_map(|(is_bid, price, qty)| Cmd::Place { is_bid, price, qty }),
            1 => (0u64..=32).prop_map(|pick| Cmd::Cancel { pick }),
            1 => (0u64..=32, 1u64..=200).prop_map(|(pick, price)| Cmd::Move { pick, price }),
        ]
    }

    /// Summary of a full replay — the determinism property compares these.
    #[derive(Debug, PartialEq, Eq)]
    struct Replay {
        fills: Vec<Fill>,
        best_bid: Option<Price>,
        best_ask: Option<Price>,
        live_orders: usize,
        resting_bids: u64,
        resting_asks: u64,
        placed_bids: u64,
        placed_asks: u64,
        filled_bids: u64,
        filled_asks: u64,
        canceled_bids: u64,
        canceled_asks: u64,
    }

    /// Run a command sequence against a fresh book, asserting the no-lock/no-
    /// cross invariant after *every* operation.
    fn replay(cmds: &[Cmd]) -> Replay {
        let mut book = OrderBook::new(SymbolId(1));
        // Live resting orders with their side — the side travels with the id
        // so the cancel ledger books quantity on the correct side.
        let mut live: Vec<(OrderId, Side)> = Vec::new();
        // Per-side ledgers: a crossing trade consumes quantity from BOTH
        // sides but produces one fill record, so conservation only holds
        // per side (found by the first property run).
        let (mut placed_bids, mut placed_asks) = (0u64, 0u64);
        let (mut filled_bids, mut filled_asks) = (0u64, 0u64);
        let (mut canceled_bids, mut canceled_asks) = (0u64, 0u64);
        let mut all_fills = Vec::new();

        for (seq, cmd) in cmds.iter().enumerate() {
            match *cmd {
                Cmd::Place { is_bid, price, qty } => {
                    let id = OrderId(seq as u64 + 1);
                    let side = if is_bid { Side::Bid } else { Side::Ask };
                    let order = Order::new(
                        id,
                        UserId(1),
                        SymbolId(1),
                        side,
                        limit(price),
                        TimeInForce::Gtc,
                        Qty::from_lots(qty).unwrap(),
                        seq as u64,
                    )
                    .unwrap();
                    let outcome = book.place(order).unwrap();
                    let filled_here: u64 = outcome.fills.iter().map(|f| f.quantity.lot()).sum();
                    // Every fill consumes its quantity from BOTH sides — the
                    // taker's remainder and each maker's lot — so both ledgers
                    // book the same sum. (Charging only the taker side broke
                    // conservation whenever a resting order was eaten as maker.)
                    filled_bids += filled_here;
                    filled_asks += filled_here;
                    if is_bid {
                        placed_bids += qty;
                    } else {
                        placed_asks += qty;
                    }
                    all_fills.extend(outcome.fills);
                    if outcome.resting.is_some() {
                        live.push((id, side));
                    }
                }
                Cmd::Cancel { pick } => {
                    if live.is_empty() {
                        continue;
                    }
                    let &(id, side) = &live[pick as usize % live.len()];
                    if let Ok(qty) = book.cancel(id) {
                        match side {
                            Side::Bid => canceled_bids += qty.lot(),
                            Side::Ask => canceled_asks += qty.lot(),
                        }
                        live.retain(|&(live_id, _)| live_id != id);
                    }
                }
                Cmd::Move { pick, price } => {
                    if live.is_empty() {
                        continue;
                    }
                    let &(id, _) = &live[pick as usize % live.len()];
                    // WouldCross rejections are fine — the invariant *is* the point.
                    let _ = book.move_order(id, Price::from_ticks(price).unwrap());
                }
            }

            // The spine invariant, after every single operation: never locked
            // (bid == ask), never crossed (bid > ask).
            if let (Some(bid), Some(ask)) = (book.best_bid(), book.best_ask()) {
                assert!(bid < ask, "book locked or crossed: bid {bid} vs ask {ask}");
            }
        }

        Replay {
            fills: all_fills,
            best_bid: book.best_bid(),
            best_ask: book.best_ask(),
            live_orders: book.len(),
            resting_bids: book.resting_lots(Side::Bid),
            resting_asks: book.resting_lots(Side::Ask),
            placed_bids,
            placed_asks,
            filled_bids,
            filled_asks,
            canceled_bids,
            canceled_asks,
        }
    }

    proptest! {
        /// Invariant 1 — no locked or crossed book, after any operation sequence.
        /// (The per-step assertion lives inside `replay`.)
        #[test]
        fn book_never_locks_or_crosses(cmds in prop::collection::vec(cmd_strategy(), 0..=60)) {
            let _ = replay(&cmds);
        }

        /// Invariant 3 — conservation, per side: a crossing trade consumes
        /// quantity from both sides but records one fill, so the ledger only
        /// balances when filled + cancelled + resting == placed on EACH side.
        #[test]
        fn quantity_is_conserved(cmds in prop::collection::vec(cmd_strategy(), 0..=60)) {
            let summary = replay(&cmds);
            prop_assert_eq!(
                summary.placed_bids,
                summary.filled_bids + summary.canceled_bids + summary.resting_bids
            );
            prop_assert_eq!(
                summary.placed_asks,
                summary.filled_asks + summary.canceled_asks + summary.resting_asks
            );
        }

        /// Invariant 4 — determinism: the same command sequence produces the
        /// identical outcome (fills and final state) on a fresh book.
        #[test]
        fn same_sequence_replays_identically(cmds in prop::collection::vec(cmd_strategy(), 0..=60)) {
            prop_assert_eq!(replay(&cmds), replay(&cmds));
        }
    }

    // Invariant 2 — price-time priority: orders placed at the same price
    // fill strictly in arrival order. (A `//` comment, not a doc comment:
    // rustdoc does not document items produced by macro invocations.)
    proptest! {
        #[test]
        fn same_level_fills_come_out_in_arrival_order(
            quantities in prop::collection::vec(1u64..=1_000, 1..=20),
        ) {
            let mut book = OrderBook::new(SymbolId(1));
            let mut arrival_order = Vec::new();
            for (i, qty) in quantities.iter().enumerate() {
                let id = OrderId(i as u64 + 1);
                book.place(gtc(id.0, Side::Bid, 1_000_000, *qty)).unwrap();
                arrival_order.push(id);
            }
            let total: u64 = quantities.iter().sum();
            // One taker sweeping the whole level: fills must mirror arrival.
            let outcome = book.place(ioc(9_999, Side::Ask, 999_999, total)).unwrap();
            let makers: Vec<OrderId> = outcome.fills.iter().map(|f| f.maker_order_id).collect();
            prop_assert_eq!(makers, arrival_order);
        }
    }
}
