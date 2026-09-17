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

// ---- reduce_order: the partial cancel (Phase 3, decision row 55) -------

#[test]
fn partial_reduce_releases_floor_cost_and_keeps_queue_position() {
    let mut engine = funded();
    // Alice rests 2.0 @ 100.0 (locks 2_000_000 exactly); Bob rests
    // 1.0 @ 99.0 AFTER her (behind her in the queue at 99.0... no —
    // at a different price. Use the same price for the position check).
    engine
        .place(gtc(1, ALICE, Side::Bid, 1_000_000, 200_000_000), None)
        .unwrap();
    engine
        .place(gtc(2, BOB, Side::Bid, 1_000_000, 100_000_000), None)
        .unwrap();
    // Reduce Alice by 0.5 base: floor(0.5 × 100.0) = 500_000 ticks out.
    let outcome = engine
        .reduce_order(OrderId(1), Qty::from_lots(50_000_000).unwrap())
        .unwrap();
    assert_eq!(outcome.reduced.lot(), 50_000_000);
    assert_eq!(outcome.remaining.lot(), 150_000_000);
    assert!(!outcome.removed);
    assert_eq!(engine.ledger().locked(ALICE, USD), 1_500_000);
    assert_eq!(
        engine.ledger().free(ALICE, USD),
        199_998_500_000, // 200B − 2M lock + 500k released
    );
    // Alice keeps queue priority — observed the only way the book's
    // public API shows it: a 1.5-lot sell at her price fills HER 1.5
    // only if she still rests ahead of Bob. Had the reduce re-queued
    // her behind Bob, this sweep would have filled Bob's 1.0 first.
    let outcome = engine
        .place(gtc(3, BOB, Side::Ask, 1_000_000, 150_000_000), None)
        .unwrap();
    let EngineOutcome::Executed { fills, resting } = outcome else {
        panic!("a crossing IOC ask is executed");
    };
    assert_eq!(
        fills.len(),
        1,
        "exactly Alice's reduced order met the sweep"
    );
    assert_eq!(fills[0].maker_order_id, OrderId(1));
    assert_eq!(fills[0].quantity.lot(), 150_000_000);
    assert_eq!(resting, None);
    // Bob's bid is untouched by both the reduce and the sweep.
    assert_eq!(engine.ledger().locked(BOB, USD), 1_000_000);
}

#[test]
fn reduce_to_zero_is_an_exact_cancel_with_dust() {
    let mut engine = funded();
    // A dusty bid: 3 lots @ 3 ticks — the Phase 0 dust case. Ceil lock
    // 1 tick holds more than the obligation (floor(settle) of any
    // split); reducing it to nothing must return the WHOLE lock.
    engine.place(gtc(1, ALICE, Side::Bid, 3, 3), None).unwrap();
    let locked_before = engine.ledger().locked(ALICE, USD);
    assert_eq!(locked_before, 1, "ceil(9/1e8)=1 tick — the dust case");
    let outcome = engine
        .reduce_order(OrderId(1), Qty::from_lots(3).unwrap())
        .unwrap();
    assert!(outcome.removed);
    assert_eq!(outcome.remaining.lot(), 0);
    assert_eq!(engine.ledger().locked(ALICE, USD), 0);
    assert_eq!(engine.ledger().free(ALICE, USD), 200_000_000_000);
    assert!(engine.book().best_bid().is_none());
}

#[test]
fn reduce_over_the_remaining_clamps_to_an_exact_cancel() {
    let mut engine = funded();
    engine
        .place(gtc(1, ALICE, Side::Bid, 1_000_000, 100_000_000), None)
        .unwrap();
    // Request 5.0 base off a 1.0 order — clamped to 1.0, removed.
    let outcome = engine
        .reduce_order(OrderId(1), Qty::from_lots(500_000_000).unwrap())
        .unwrap();
    assert_eq!(outcome.reduced.lot(), 100_000_000);
    assert!(outcome.removed);
    assert_eq!(engine.ledger().locked(ALICE, USD), 0);
    assert_eq!(engine.ledger().free(ALICE, USD), 200_000_000_000);
}

#[test]
fn ask_reduce_releases_exactly_the_reduced_lots() {
    let mut engine = funded();
    engine
        .place(gtc(1, ALICE, Side::Ask, 1_000_000, 200_000_000), None)
        .unwrap();
    let outcome = engine
        .reduce_order(OrderId(1), Qty::from_lots(50_000_000).unwrap())
        .unwrap();
    assert!(!outcome.removed);
    // Asks lock base lots exactly; the release is exact too.
    assert_eq!(engine.ledger().locked(ALICE, BTC), 150_000_000);
    assert_eq!(engine.ledger().free(ALICE, BTC), 350_000_000);
}

#[test]
fn reduce_of_unknown_or_dead_id_rejects_with_nothing_moved() {
    let mut engine = funded();
    assert!(matches!(
        engine.reduce_order(OrderId(99), Qty::from_lots(1).unwrap()),
        Err(EngineError::Book(BookError::UnknownOrder {
            id: OrderId(99)
        }))
    ));
    // A canceled (dead) id too.
    engine
        .place(gtc(1, ALICE, Side::Bid, 1_000_000, 100_000_000), None)
        .unwrap();
    engine.cancel(OrderId(1)).unwrap();
    assert!(
        engine
            .reduce_order(OrderId(1), Qty::from_lots(1).unwrap())
            .is_err()
    );
    assert_eq!(engine.ledger().locked(ALICE, USD), 0);
}

#[test]
fn split_releases_conserve_the_whole_lock() {
    let mut engine = funded();
    // A dusty bid whose ceil lock holds dust: 3 lots @ 1_000_001 ticks.
    // Lock = ceil(3 × 1_000_001 / 1e8) = 1 tick; each 1-lot reduce
    // releases floor(1 × 1_000_001 / 1e8) = 0 ticks — the dust stays
    // with the remainder, exactly as settlement's floor would leave it.
    engine
        .place(gtc(1, ALICE, Side::Bid, 1_000_001, 3), None)
        .unwrap();
    assert_eq!(engine.ledger().locked(ALICE, USD), 1);
    engine
        .reduce_order(OrderId(1), Qty::from_lots(1).unwrap())
        .unwrap();
    assert_eq!(
        engine.ledger().locked(ALICE, USD),
        1,
        "floor split released 0"
    );
    engine
        .reduce_order(OrderId(1), Qty::from_lots(1).unwrap())
        .unwrap();
    assert_eq!(
        engine.ledger().locked(ALICE, USD),
        1,
        "still holding the dust"
    );
    // The final 1 lot by cancel: the whole lock (dust included) returns.
    engine.cancel(OrderId(1)).unwrap();
    assert_eq!(engine.ledger().locked(ALICE, USD), 0);
    assert_eq!(engine.ledger().free(ALICE, USD), 200_000_000_000);
}

// ---- Position limits through the engine (Phase 3, row 54) ---------------

#[test]
fn place_beyond_the_position_limit_never_reaches_the_book() {
    let mut engine = funded();
    engine
        .ledger_mut()
        .set_position_limit(ALICE, USD, 1_500_000);
    // 2.0 base @ 100.0 = 2_000_000 quote ticks — over the 1_500_000
    // cap, trivially under free. The cap rejects; the book never sees it.
    assert!(matches!(
        engine.place(gtc(1, ALICE, Side::Bid, 1_000_000, 200_000_000), None),
        Err(EngineError::Risk(RiskError::PositionLimitExceeded {
            cap: 1_500_000,
            ..
        }))
    ));
    assert!(engine.book().best_bid().is_none(), "the book never saw it");
    assert_eq!(engine.ledger().locked(ALICE, USD), 0);
}

#[test]
fn bid_move_into_the_cap_is_rejected_with_nothing_moved() {
    let mut engine = funded();
    // 1.0 base @ 100.0 locks 1_000_000 quote ticks: inside the cap.
    engine
        .place(gtc(1, ALICE, Side::Bid, 1_000_000, 100_000_000), None)
        .unwrap();
    engine
        .ledger_mut()
        .set_position_limit(ALICE, USD, 1_500_000);
    // Move to 200.0 would need 2_000_000 — past the cap. Funding would
    // pass (free is huge); the cap rejects before anything moves.
    assert!(matches!(
        engine.move_order(OrderId(1), Price::from_ticks(2_000_000).unwrap()),
        Err(EngineError::Risk(RiskError::PositionLimitExceeded {
            cap: 1_500_000,
            ..
        }))
    ));
    // Nothing moved: lock, price, book state all as before.
    assert_eq!(engine.ledger().locked(ALICE, USD), 1_000_000);
    assert_eq!(
        engine.live_orders()[&OrderId(1)].price,
        Some(Price::from_ticks(1_000_000).unwrap())
    );
}

#[test]
fn bid_move_within_the_cap_succeeds() {
    let mut engine = funded();
    engine
        .place(gtc(1, ALICE, Side::Bid, 1_000_000, 100_000_000), None)
        .unwrap(); // locks 1_000_000
    engine
        .ledger_mut()
        .set_position_limit(ALICE, USD, 1_500_000);
    engine
        .move_order(OrderId(1), Price::from_ticks(1_200_000).unwrap())
        .unwrap(); // 1_200_000 ≤ 1_500_000
    assert_eq!(engine.ledger().locked(ALICE, USD), 1_200_000);
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
