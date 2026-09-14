//! Integration test: the full order lifecycle through the real [`Engine`] —
//! the Phase 1 component the `order_lifecycle.rs` test previously *played*
//! by hand (commit/settle/release choreography).
//!
//! These tests exercise cross-module behavior through public APIs only:
//! the engine's `place` / `cancel` / `move_order`, the ledger's read
//! accessors, and the book's best-price accessors. The assertion style is
//! the one the integration-testing skill prescribes: every expected balance
//! is derived from the scenario arithmetic in a comment, none asserted
//! blind. Conservation is asserted from the outside (Σ free + locked =
//! deposits, per currency).
//!
//! Scales (see `domain.rs`): `PRICE_SCALE = 10_000` ticks per unit,
//! `QTY_SCALE = 100_000_000` lots per unit. Prices/quantities below are in
//! raw ticks/lots: 1_000_000 ticks = 100.0 quote, 100_000_000 lots = 1.0
//! base.

use launchpad_core::domain::{
    CurrencyId, Order, OrderId, OrderType, Price, Qty, Side, SymbolId, TimeInForce, UserId,
};
use launchpad_core::engine::{Engine, EngineError, EngineOutcome};
use launchpad_core::risk::RiskError;

const USD: CurrencyId = CurrencyId(1); // quote
const BTC: CurrencyId = CurrencyId(2); // base
const ALICE: UserId = UserId(1);
const BOB: UserId = UserId(2);

/// A fresh engine for the BTC/USD pair, both users funded with
/// 20_000_000_000 ticks (2000.0) quote and 500_000_000 lots (5.0) base.
fn funded() -> Engine {
    let mut engine = Engine::new(SymbolId(1), USD, BTC);
    for user in [ALICE, BOB] {
        engine
            .ledger_mut()
            .deposit(user, USD, 20_000_000_000)
            .unwrap();
        engine.ledger_mut().deposit(user, BTC, 500_000_000).unwrap();
    }
    engine
}

/// GTC limit order with id == timestamp (the usual test shape).
fn gtc(id: u64, user: UserId, side: Side, price_ticks: u64, lots: u64) -> Order {
    Order::new(
        OrderId(id),
        user,
        SymbolId(1),
        side,
        OrderType::Limit {
            price: Price::from_ticks(price_ticks).unwrap(),
        },
        TimeInForce::Gtc,
        Qty::from_lots(lots).unwrap(),
        id,
    )
    .unwrap()
}

/// Market order (IOC-only by domain rule).
fn market(id: u64, user: UserId, side: Side, lots: u64) -> Order {
    Order::new(
        OrderId(id),
        user,
        SymbolId(1),
        side,
        OrderType::Market,
        TimeInForce::Ioc,
        Qty::from_lots(lots).unwrap(),
        id,
    )
    .unwrap()
}

/// Σ (free + locked) over both users, one currency — the outside view of
/// conservation: trading moves value, never mints it.
fn total(engine: &Engine, currency: CurrencyId) -> u64 {
    [ALICE, BOB]
        .iter()
        .map(|u| engine.ledger().free(*u, currency) + engine.ledger().locked(*u, currency))
        .sum()
}

/// Full lifecycle, engine-orchestrated: deposit → place (locks) → sweep
/// (settles both legs at maker prices) → GTC remainder rests → cancel
/// releases the whole remaining lock.
#[test]
fn full_order_lifecycle_through_the_engine() {
    let mut engine = funded();

    // Alice asks 2.0 base @ 100.0: locks 200_000_000 base lots.
    engine
        .place(gtc(1, ALICE, Side::Ask, 1_000_000, 200_000_000), None)
        .unwrap();
    assert_eq!(engine.ledger().locked(ALICE, BTC), 200_000_000);
    assert_eq!(engine.ledger().free(ALICE, BTC), 300_000_000); // 5.0 − 2.0

    // Bob bids 1.0 base @ 100.0: locks ceil(100M × 1M / 1e8) = 1_000_000
    // ticks (exact). The sweep fills fully at the maker's price and
    // settles both legs atomically — no hand choreography here.
    let outcome = engine
        .place(gtc(2, BOB, Side::Bid, 1_000_000, 100_000_000), None)
        .unwrap();
    let EngineOutcome::Executed { fills, resting } = outcome else {
        panic!("a crossing GTC bid executes, not kills");
    };
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].maker_order_id, OrderId(1));
    assert_eq!(fills[0].taker_order_id, OrderId(2));
    assert_eq!(fills[0].price, Price::from_ticks(1_000_000).unwrap());
    assert_eq!(fills[0].quantity, Qty::from_lots(100_000_000).unwrap());
    assert_eq!(resting, None, "the bid fully filled");

    // Alice: −1.0 base (delivered to Bob), +100.0 quote free, 1.0 still
    // locked in the resting remainder.
    assert_eq!(engine.ledger().free(ALICE, USD), 20_001_000_000);
    assert_eq!(engine.ledger().locked(ALICE, BTC), 100_000_000);
    // Bob: quote lock settled away (1_000_000 → Alice), +1.0 base free.
    assert_eq!(engine.ledger().locked(BOB, USD), 0);
    assert_eq!(engine.ledger().free(BOB, USD), 19_999_000_000);
    assert_eq!(engine.ledger().free(BOB, BTC), 600_000_000);

    // Conservation, the outside view.
    assert_eq!(total(&engine, USD), 40_000_000_000);
    assert_eq!(total(&engine, BTC), 1_000_000_000);

    // Cancel the remainder: the whole remaining lock (dust included —
    // exact here) releases.
    engine.cancel(OrderId(1)).unwrap();
    assert_eq!(engine.ledger().locked(ALICE, BTC), 0);
    assert_eq!(engine.ledger().free(ALICE, BTC), 400_000_000); // 5.0 − 1.0
}

/// A self-trade is both fill legs on one account: locks must return to
/// free, never evaporate — conservation from the outside catches minting
/// or burning.
#[test]
fn self_trade_neither_mints_nor_burns() {
    let mut engine = funded();

    // Alice asks 1.0 @ 100.0, then bids 1.0 @ 100.0 crossing herself.
    engine
        .place(gtc(1, ALICE, Side::Ask, 1_000_000, 100_000_000), None)
        .unwrap();
    engine
        .place(gtc(2, ALICE, Side::Bid, 1_000_000, 100_000_000), None)
        .unwrap();

    // The quote leg pays herself (settles her own lock back to free) and
    // the base leg likewise: balances return to the starting point.
    assert_eq!(engine.ledger().free(ALICE, USD), 20_000_000_000);
    assert_eq!(engine.ledger().locked(ALICE, USD), 0);
    assert_eq!(engine.ledger().free(ALICE, BTC), 500_000_000);
    assert_eq!(engine.ledger().locked(ALICE, BTC), 0);

    // The outside view is unchanged: a self-trade is an internal reshuffle.
    assert_eq!(total(&engine, USD), 40_000_000_000);
    assert_eq!(total(&engine, BTC), 1_000_000_000);
    assert!(engine.live_orders().is_empty());
}

/// The engine's risk gate composes the ledger's: an unfundable order is
/// rejected before the book sees it, and nothing moves anywhere.
#[test]
fn unfundable_order_never_reaches_the_book() {
    let mut engine = Engine::new(SymbolId(1), USD, BTC);
    engine.ledger_mut().deposit(BOB, USD, 1_000_000).unwrap(); // 100.0 quote

    // A bid for 2.0 base @ 100.0 needs 200.0 quote — more than free.
    let error = engine
        .place(gtc(1, BOB, Side::Bid, 1_000_000, 200_000_000), None)
        .unwrap_err();
    assert!(matches!(
        error,
        EngineError::Risk(RiskError::InsufficientForOrder { .. })
    ));
    assert!(engine.book().is_empty(), "the book never saw the order");
    assert!(engine.live_orders().is_empty());
    assert_eq!(engine.ledger().free(BOB, USD), 1_000_000);
    assert_eq!(engine.ledger().locked(BOB, USD), 0);
}

/// A market bid must carry a reserve (there is nothing else to lock
/// against); with one, it sweeps up to the reserve and releases the
/// unused lock with the dead remainder.
#[test]
fn market_bid_requires_reserve_then_sweeps_bounded() {
    let mut engine = funded();

    // Without a reserve: rejected, nothing anywhere moves.
    let error = engine
        .place(market(1, BOB, Side::Bid, 100_000_000), None)
        .unwrap_err();
    assert_eq!(error, EngineError::MarketBidRequiresReserve);
    assert!(engine.book().is_empty());
    assert_eq!(engine.ledger().free(BOB, USD), 20_000_000_000);

    // Alice rests 0.3 base @ 100.0: locks 30_000_000 lots.
    engine
        .place(gtc(2, ALICE, Side::Ask, 1_000_000, 30_000_000), None)
        .unwrap();

    // Bob's market bid for 1.0 base with reserve 150.0: locks
    // ceil(100M × 1.5M / 1e8) = 1_500_000 ticks, sweeps 0.3 at the maker's
    // 100.0 (settling 300_000 ticks), remainder dies, leftover lock
    // 1_200_000 returns to free.
    let outcome = engine
        .place(
            market(3, BOB, Side::Bid, 100_000_000),
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
    assert_eq!(
        engine.ledger().free(BOB, USD),
        20_000_000_000 - 300_000, // settled the sweep
        "the leftover 1.2M-tick lock returned to free"
    );
    assert_eq!(engine.ledger().free(BOB, BTC), 530_000_000); // +0.3 base
    assert_eq!(engine.ledger().free(ALICE, USD), 20_000_300_000);
    assert_eq!(engine.ledger().locked(ALICE, BTC), 0);
    assert!(engine.live_orders().is_empty(), "both orders are dead");
    assert_eq!(total(&engine, USD), 40_000_000_000);
    assert_eq!(total(&engine, BTC), 1_000_000_000);
}
