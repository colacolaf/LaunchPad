//! Journal tests (decision row 59): replay identity is the Done-when, and
//! divergence is loud. Every test drives only the public API — engine,
//! journal, digest — the same surface the venue will get.

use super::*;
use crate::domain::{OrderType, TimeInForce};
use proptest::prelude::*;

const SYMBOL: SymbolId = SymbolId(1);
const USD: CurrencyId = CurrencyId(1);
const BTC: CurrencyId = CurrencyId(2);
const ALICE: UserId = UserId(1);
const BOB: UserId = UserId(2);

/// House scales: prices near 1_000_000 ticks, quantities in multiples of
/// 50_000_000 lots (0.5 base units at `QTY_SCALE = 1e8`).
fn price(ticks: u64) -> Price {
    Price::from_ticks(ticks).unwrap()
}

fn lots(units_x2: u64) -> Qty {
    Qty::from_lots(units_x2 * 50_000_000).unwrap()
}

fn gtc_place(id: u64, user: UserId, side: Side, ticks: u64, units_x2: u64) -> Command {
    Command::Place {
        order: Order::new(
            OrderId(id),
            user,
            SYMBOL,
            side,
            OrderType::Limit {
                price: price(ticks),
            },
            TimeInForce::Gtc,
            lots(units_x2),
            id, // submission sequence; the book never reads it
        )
        .unwrap(),
        reserve: None,
    }
}

fn deposit(user: UserId, currency: CurrencyId, amount: u64) -> Command {
    Command::Deposit {
        user,
        currency,
        amount,
    }
}

/// Record `commands` against a fresh fee'd engine, then replay the journal
/// into a brand-new engine and compare the deep digests.
fn assert_replay_identity(commands: &[Command]) -> (u64, u64) {
    let mut engine = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    let mut journal = Journal::for_engine(&engine);
    for command in commands {
        journal.record(&mut engine, *command);
    }
    let before = deep_digest(&engine);
    let replayed = journal.replay().unwrap();
    let after = deep_digest(&replayed);
    (before, after)
}

// ---- The Done-when ------------------------------------------------------

#[test]
fn replay_produces_identical_state_hand_scenario() {
    // Deposits → resting pair → reduce → move (top-up) → crossing IOC
    // (fills + fee legs) → cancel remainder → crossing GTC full fill.
    // Both fees ON and every operation kind in one stream.
    let commands = vec![
        deposit(ALICE, USD, 200_000_000_000),
        deposit(ALICE, BTC, 500_000_000),
        deposit(BOB, USD, 200_000_000_000),
        deposit(BOB, BTC, 500_000_000),
        gtc_place(1, ALICE, Side::Bid, 1_000_000, 3), // rests (1.5 base bid)
        gtc_place(2, BOB, Side::Ask, 1_001_000, 3),   // rests above the bid
        Command::Reduce {
            id: OrderId(1),
            by: lots(1),
        }, // 1.5 → 0.5, queue kept
        Command::Move {
            id: OrderId(1),
            new_price: price(1_000_500),
        }, // top-up path
        Command::Place {
            order: Order::new(
                OrderId(3),
                ALICE,
                SYMBOL,
                Side::Bid,
                OrderType::Limit {
                    price: price(1_002_000),
                },
                TimeInForce::Ioc,
                lots(1),
                3,
            )
            .unwrap(),
            reserve: None,
        }, // sweeps BOB's ask: fill @ 1_001_000, fees on both legs
        Command::Cancel { id: OrderId(2) },           // BOB's remainder (1.0) released
        gtc_place(4, BOB, Side::Ask, 1_000_000, 2),   // crosses ALICE: full fill @ 1_000_500
    ];
    let (before, after) = assert_replay_identity(&commands);
    assert_eq!(before, after, "replay must reproduce the state exactly");

    // Sanity: the scenario must actually have exercised fees and fills —
    // the identity claim is worthless if the stream never traded. Both
    // crossings fully fill: the book ends empty, the fee sink does not.
    let mut engine = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    let mut journal = Journal::for_engine(&engine);
    for command in &commands {
        journal.record(&mut engine, *command);
    }
    assert!(
        engine.ledger().fees_collected(USD) > 0,
        "fills must have collected quote fees"
    );
    assert!(
        engine.ledger().fees_collected(BTC) > 0,
        "fills must have collected base fees"
    );
    assert!(
        engine.book().is_empty(),
        "both crossings fully fill; nothing may rest"
    );
}

#[test]
fn empty_journal_replays_to_an_equally_empty_engine() {
    let engine = Engine::new(SYMBOL, USD, BTC);
    let journal = Journal::for_engine(&engine);
    let replayed = journal.replay().unwrap();
    assert_eq!(deep_digest(&engine), deep_digest(&replayed));
    assert_eq!(journal.header, Some(JournalHeader::of(&replayed)));
    assert!(journal.entries.is_empty());
}

// ---- Rejections are journaled too ---------------------------------------

#[test]
fn rejected_commands_are_journaled_and_replay_agrees() {
    let mut engine = Engine::new(SYMBOL, USD, BTC);
    let mut journal = Journal::for_engine(&engine);
    let commands = [
        deposit(ALICE, USD, 1_000_000_000),
        Command::SetPositionLimit {
            user: ALICE,
            currency: USD,
            cap: 500_000,
        },
        gtc_place(1, ALICE, Side::Bid, 1_000_000, 2), // cost 1_000_000 > cap: REJECTED
        Command::Withdraw {
            user: ALICE,
            currency: USD,
            amount: 2_000_000_000,
        }, // > free: REJECTED
        gtc_place(2, ALICE, Side::Bid, 1_000_000, 1), // cost 500_000 = cap: accepted
        gtc_place(3, ALICE, Side::Bid, 1_000_000, 1), // would_lock 1_000_000 > cap: REJECTED
        Command::ClearPositionLimit {
            user: ALICE,
            currency: USD,
        },
        gtc_place(4, ALICE, Side::Bid, 1_000_000, 1), // uncapped + funded: accepted
    ];
    let mut accepted = Vec::new();
    for command in commands {
        accepted.push(journal.record(&mut engine, command));
    }
    // The test is only honest if the stream really mixed outcomes:
    assert_eq!(
        accepted,
        vec![true, true, false, false, true, false, true, true],
        "expected the scripted accept/reject pattern"
    );
    let (before, after) = (
        deep_digest(&engine),
        deep_digest(&journal.replay().unwrap()),
    );
    assert_eq!(before, after, "rejections must replay to the same state");
}

#[test]
fn forged_acceptance_is_divergence_not_silence() {
    let mut engine = Engine::new(SYMBOL, USD, BTC);
    let mut journal = Journal::for_engine(&engine);
    journal.record(&mut engine, deposit(ALICE, USD, 1_000_000_000));
    journal.record(&mut engine, gtc_place(1, ALICE, Side::Bid, 1_000_000, 2));
    // Corrupt the record: the deposit was accepted, the journal now lies.
    journal.entries[0].accepted = false;
    let error = journal.replay().unwrap_err();
    assert_eq!(
        error,
        JournalError::Divergence {
            index: 0,
            recorded: false,
            replayed: true,
        }
    );
}

#[test]
fn header_mismatch_is_refused() {
    let engine = Engine::new(SYMBOL, USD, BTC);
    let journal = Journal::for_engine(&engine);
    // Wrong fees on the target engine.
    let mut other = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    assert!(matches!(
        journal.replay_into(&mut other),
        Err(JournalError::HeaderMismatch { .. })
    ));
    // No header at all.
    let bare = Journal::new();
    let mut any = Engine::new(SYMBOL, USD, BTC);
    assert!(matches!(
        bare.replay_into(&mut any),
        Err(JournalError::HeaderMismatch { .. })
    ));
}

// ---- The deep digest bites (the Phase 1 carry-forward) -------------------

#[test]
fn depth_accessor_sees_levels_and_queue_order() {
    let mut engine = Engine::new(SYMBOL, USD, BTC);
    let mut journal = Journal::for_engine(&engine);
    for command in [
        deposit(ALICE, USD, 10_000_000_000),
        deposit(ALICE, BTC, 10_000_000_000), // asks lock base — without this every ask dies at risk
        deposit(BOB, USD, 10_000_000_000),
        deposit(BOB, BTC, 10_000_000_000),
        gtc_place(1, ALICE, Side::Bid, 1_000_000, 2),
        gtc_place(2, BOB, Side::Bid, 1_000_000, 1), // same level, behind Alice
        gtc_place(3, ALICE, Side::Bid, 999_000, 1), // worse level
        gtc_place(4, BOB, Side::Ask, 1_001_000, 1),
        gtc_place(5, ALICE, Side::Ask, 1_002_000, 1),
    ] {
        journal.record(&mut engine, command);
    }
    let bids = engine.book().depth(Side::Bid);
    assert_eq!(
        bids,
        vec![
            (
                price(1_000_000),
                vec![
                    (OrderId(1), ALICE, 100_000_000),
                    (OrderId(2), BOB, 50_000_000)
                ]
            ),
            (price(999_000), vec![(OrderId(3), ALICE, 50_000_000)]),
        ],
        "best level first, queue in arrival order"
    );
    let asks = engine.book().depth(Side::Ask);
    assert_eq!(
        asks,
        vec![
            (price(1_001_000), vec![(OrderId(4), BOB, 50_000_000)]),
            (price(1_002_000), vec![(OrderId(5), ALICE, 50_000_000)]),
        ],
        "asks: best (lowest) first"
    );
}

#[test]
fn deep_digest_distinguishes_queue_order_not_just_aggregates() {
    // Same two orders, same totals, opposite arrival order at one price.
    // An aggregate fold would call these identical; the deep fold must not.
    let build = |swap: bool| {
        let mut engine = Engine::new(SYMBOL, USD, BTC);
        let mut journal = Journal::for_engine(&engine);
        journal.record(&mut engine, deposit(ALICE, USD, 10_000_000_000));
        journal.record(&mut engine, deposit(BOB, USD, 10_000_000_000));
        let (first, second) = if swap { (2u64, 1u64) } else { (1, 2) };
        journal.record(
            &mut engine,
            gtc_place(first, ALICE, Side::Bid, 1_000_000, 1),
        );
        journal.record(&mut engine, gtc_place(second, BOB, Side::Bid, 1_000_000, 1));
        engine
    };
    assert_ne!(
        deep_digest(&build(false)),
        deep_digest(&build(true)),
        "price-time priority must be digest-observable"
    );
}

// ---- The Done-when, property edition -------------------------------------

/// A command drawn from the biased universe: early deposits are likely
/// (else nothing is accepted), prices hover in a narrow band (so crosses
/// happen), ids recycle (dead-id rejections exercised), and every
/// operation kind appears.
fn cmd_strategy() -> impl Strategy<Value = Command> {
    // Deposits only, and never overflowing: ≤120 draws × <1e11 each is
    // ~1e13 per (user, currency) — far below u64::MAX. Withdraws are
    // deliberately excluded: an over-free withdrawal is *accepted=false*
    // now (recorded outcome), but a stream dominated by refused funding is
    // dead coverage; the scripted rejection test covers withdrawals.
    let funding =
        (0u64..3, 0u64..3, 1u64..100_000_000_000).prop_map(|(u, c, amount)| Command::Deposit {
            user: UserId(u + 1),
            currency: CurrencyId(c + 1),
            amount,
        });
    let order_kind = (0u8..6, 0u64..3, 1u64..=10, 0u64..3, 1u64..4, 1u64..4).prop_map(
        |(kind, u, id, price_idx, mult, by_mult)| {
            let user = UserId(u + 1);
            let id = OrderId(id);
            let ticks = [900_000, 1_000_000, 1_100_000][price_idx as usize];
            match kind {
                0 => Command::Cancel { id },
                1 => Command::Reduce {
                    id,
                    by: lots(by_mult * 2),
                },
                2 => Command::Move {
                    id,
                    new_price: price(ticks),
                },
                3 => Command::SetPositionLimit {
                    user,
                    currency: CurrencyId(1),
                    // 1–3M caps: place costs run 450k–1.65M ticks, so
                    // caps genuinely bite on some draws.
                    cap: mult * 1_000_000,
                },
                4 => Command::ClearPositionLimit {
                    user,
                    currency: CurrencyId(1),
                },
                _ => Command::Place {
                    order: Order::new(
                        id,
                        user,
                        SYMBOL,
                        if price_idx == 0 { Side::Bid } else { Side::Ask },
                        OrderType::Limit {
                            price: price(ticks),
                        },
                        TimeInForce::Ioc,
                        lots(mult * 2),
                        id.0,
                    )
                    .unwrap(),
                    reserve: None,
                },
            }
        },
    );
    prop_oneof![3 => funding, 2 => gtc_strategy(), 2 => order_kind]
}

fn gtc_strategy() -> impl Strategy<Value = Command> {
    (0u64..3, 0u64..2, 1u64..=10, 0u64..3, 1u64..4).prop_map(|(u, s, id, price_idx, mult)| {
        Command::Place {
            order: Order::new(
                OrderId(id),
                UserId(u + 1),
                SYMBOL,
                if s == 0 { Side::Bid } else { Side::Ask },
                OrderType::Limit {
                    price: price([900_000, 1_000_000, 1_100_000][price_idx as usize]),
                },
                TimeInForce::Gtc,
                lots(mult * 2),
                id,
            )
            .unwrap(),
            reserve: None,
        }
    })
}

proptest! {
    /// **The Done-when, proven:** a random command stream recorded against
    /// one engine replays into a fresh engine whose deep digest — full
    /// money state, live orders, and book depth with queue order — is
    /// byte-identical. Rejections included: they are entries too.
    #[test]
    fn replay_of_random_commands_produces_identical_state(
        cmds in prop::collection::vec(cmd_strategy(), 1..=120),
    ) {
        let (before, after) = assert_replay_identity(&cmds);
        prop_assert_eq!(before, after);
    }
}
