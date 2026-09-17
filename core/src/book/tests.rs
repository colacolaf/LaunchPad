//! Book tests (verbatim extract of the former inline module).

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
