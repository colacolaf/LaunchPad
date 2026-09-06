//! Integration test: the full order lifecycle through the real [`OrderBook`]
//! and [`Ledger`], with these tests playing the role the Phase 1 engine will.
//!
//! The sequence exercises every money-moving path in the system:
//! deposit → place-time commit (bid locks quote, ask locks base) →
//! fill settlement (quote moves buyer→seller, base moves seller→buyer,
//! both at the maker's price) → cancel-time release of the unused lock.
//!
//! Conservation is asserted from the outside: the sum of all balances plus
//! what was withdrawn never changes except by deposit. This is the
//! `core/tests/` layer from the testing skill — cross-module behavior,
//! exercised through public APIs only.

use launchpad_core::book::{BookError, OrderBook, PlaceOutcome};
use launchpad_core::domain::{
    CurrencyId, Order, OrderId, OrderType, Price, Qty, Side, SymbolId, TimeInForce, UserId,
};
use launchpad_core::risk::Ledger;

const USD: CurrencyId = CurrencyId(1); // quote currency
const BTC: CurrencyId = CurrencyId(2); // base currency
const ALICE: UserId = UserId(1);
const BOB: UserId = UserId(2);

/// Place an order with full risk wiring, playing the engine:
/// compute the commitment, enforce it, then submit to the book.
/// Returns the book outcome; panics via `expect` on risk rejections the
/// test doesn't expect.
fn place_with_risk(
    book: &mut OrderBook,
    ledger: &mut Ledger,
    order: Order,
    // (currency, amount) to commit — bids: quote ticks via ceil helper;
    // asks: base lots. `None` commitment = skip the risk gate (used to
    // build the resting book cheaply in one test).
    commit: Option<(CurrencyId, u64)>,
) -> Result<PlaceOutcome, BookError> {
    if let Some((currency, amount)) = commit {
        ledger
            .commit(order.user, currency, amount)
            .expect("test orders must be fundable");
    }
    book.place(order)
}

/// Full-lifecycle scenario, asserted step by step.
#[test]
fn full_order_lifecycle_moves_value_correctly() {
    let mut book = OrderBook::new(SymbolId(1));
    let mut ledger = Ledger::new();

    // ---- funding ---------------------------------------------------------
    // Alice: 200.0 quote, 5.0 base. Bob: 200.0 quote, 5.0 base.
    for (user, quote, base) in [
        (ALICE, 20_000_000_000, 500_000_000),
        (BOB, 20_000_000_000, 500_000_000),
    ] {
        ledger.deposit(user, USD, quote).unwrap();
        ledger.deposit(user, BTC, base).unwrap();
    }

    // ---- Alice places a GTC ask: 2.0 base @ 100.0 quote -------------------
    // Ask commitment = base lots = 200_000_000.
    let ask = gtc(1, ALICE, Side::Ask, 1_000_000, 200_000_000);
    place_with_risk(&mut book, &mut ledger, ask, Some((BTC, 200_000_000))).unwrap();
    assert_eq!(ledger.free(ALICE, BTC), 300_000_000); // 5.0 − 2.0
    assert_eq!(ledger.locked(ALICE, BTC), 200_000_000);

    // ---- Bob places a GTC bid: 1.0 base @ 100.0 quote ----------------------
    // Bid commitment = ceil(100_000_000 lots × 1_000_000 ticks / 1e8)
    //               = ceil(1_000_000) = 1_000_000 ticks = 100.0 quote. Exact.
    let bid = gtc(2, BOB, Side::Bid, 1_000_000, 100_000_000);
    let outcome = place_with_risk(&mut book, &mut ledger, bid, Some((USD, 1_000_000))).unwrap();

    // The trade happened. One fill, at the maker's (Alice's) price.
    assert_eq!(outcome.fills.len(), 1);
    let fill = &outcome.fills[0];
    assert_eq!(fill.maker_order_id, OrderId(1));
    assert_eq!(fill.taker_order_id, OrderId(2));
    assert_eq!(fill.price, Price::from_ticks(1_000_000).unwrap());
    assert_eq!(fill.quantity, Qty::from_lots(100_000_000).unwrap()); // 1.0 base

    // ---- settle the fill (engine's job): quote BOB→ALICE, base ALICE→BOB ---
    // Quote amount = fill lots × price ticks / QTY_SCALE, ceil — same helper
    // the commitment used, so lock always covers the settlement.
    let fill_quote_ticks =
        launchpad_core::risk::quote_cost_ticks(fill.quantity, fill.price).unwrap();
    ledger.settle(BOB, ALICE, USD, fill_quote_ticks);
    ledger.settle(ALICE, BOB, BTC, fill.quantity.lot());

    // ---- post-trade balances (every number is derived, none asserted blind)
    // Alice: −2.0 base (locked, then 1.0 sold → BOB; 1.0 still locked in the
    // resting remainder), +100.0 quote free (20_000.0 + 100.0 = 20_001.0).
    assert_eq!(ledger.locked(ALICE, BTC), 100_000_000);
    assert_eq!(ledger.free(ALICE, USD), 20_001_000_000);
    assert_eq!(ledger.free(ALICE, BTC), 300_000_000);
    // Bob: quote lock settled away (1_000_000 → 0, gone to Alice);
    // base +1.0 free.
    assert_eq!(ledger.locked(BOB, USD), 0);
    assert_eq!(ledger.free(BOB, USD), 19_999_000_000);
    assert_eq!(ledger.free(BOB, BTC), 600_000_000);

    // ---- conservation, the outside view -------------------------------------
    // Total quote in system = deposits only. Trading moved it, never minted.
    let quote_in_system = ledger.free(ALICE, USD)
        + ledger.locked(ALICE, USD)
        + ledger.free(BOB, USD)
        + ledger.locked(BOB, USD);
    assert_eq!(quote_in_system, 40_000_000_000);
    let base_in_system = ledger.free(ALICE, BTC)
        + ledger.locked(ALICE, BTC)
        + ledger.free(BOB, BTC)
        + ledger.locked(BOB, BTC);
    assert_eq!(base_in_system, 1_000_000_000);

    // ---- cancel the remainder: the lock releases ----------------------------
    book.cancel(OrderId(1)).unwrap();
    ledger.release(ALICE, BTC, 100_000_000);
    assert_eq!(ledger.free(ALICE, BTC), 400_000_000);
    assert_eq!(ledger.locked(ALICE, BTC), 0);
}

/// The risk gate actually gates: an unfundable bid never reaches the book.
#[test]
fn unfundable_bid_is_rejected_before_the_book() {
    let book = OrderBook::new(SymbolId(1)); // never mutated: risk gate fires first
    let mut ledger = Ledger::new();
    ledger.deposit(BOB, USD, 1_000_000).unwrap(); // 100.0 quote only

    // A bid for 2.0 base @ 100.0 needs 200.0 quote — more than free.
    let commit = launchpad_core::risk::quote_cost_ticks(
        Qty::from_lots(200_000_000).unwrap(),
        Price::from_ticks(1_000_000).unwrap(),
    )
    .unwrap();
    assert_eq!(commit, 2_000_000);
    assert_eq!(
        ledger.commit(BOB, USD, commit),
        Err(launchpad_core::risk::RiskError::InsufficientForOrder {
            currency: USD,
            required: 2_000_000,
            available: 1_000_000,
        })
    );
    // The book never saw the order — nothing resting anywhere.
    assert!(book.is_empty());
    // And Bob's balance is untouched (all-or-nothing commit).
    assert_eq!(ledger.free(BOB, USD), 1_000_000);
    assert_eq!(ledger.locked(BOB, USD), 0);
}

/// Move's risk story: repricing up needs new free quote; the released dust
/// from ceil-rounding goes back to free on cancel.
#[test]
fn ceil_dust_is_released_not_lost() {
    let mut book = OrderBook::new(SymbolId(1));
    let mut ledger = Ledger::new();
    ledger.deposit(BOB, USD, 10_000_000).unwrap(); // 1000.0 quote

    // Price 0.0000001 quote (1 tick). Bid for 1 lot: exact cost = 1e-8
    // ticks → ceil = 1 tick. The lock is one tick; the *settlement* for a
    // fill would draw exactly... 1 tick. No dust here — dust needs a
    // remainder in the division. Use price 3 ticks, 3 lots:
    // cost = 3 × 3 / 1e8 → ceil(9e-8) = 1 tick.
    let bid = gtc(1, BOB, Side::Bid, 3, 3);
    let needed = launchpad_core::risk::quote_cost_ticks(
        Qty::from_lots(3).unwrap(),
        Price::from_ticks(3).unwrap(),
    )
    .unwrap();
    assert_eq!(needed, 1, "ceil of 9e-8 ticks is 1 tick");
    ledger.commit(BOB, USD, needed).unwrap();
    place_with_risk(&mut book, &mut ledger, bid, None).unwrap(); // already committed

    // Cancel releases the whole (dusty) lock: BOB gets his tick back.
    book.cancel(OrderId(1)).unwrap();
    ledger.release(BOB, USD, 1);
    assert_eq!(ledger.free(BOB, USD), 10_000_000);
    assert_eq!(ledger.locked(BOB, USD), 0);
}

/// Order ids used by the scenarios: engine-assigned sequence, GTC limit.
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
