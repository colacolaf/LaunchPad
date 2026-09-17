//! Journal tests (decision row 59): replay identity is the Done-when, and
//! divergence is loud. Every test drives only the public API — engine,
//! journal, digest — the same surface the venue will get.

use super::*;
use crate::book::BookError;
use crate::domain::{OrderType, TimeInForce};
use crate::engine::{EngineError, SnapshotOrder};
use crate::risk::RiskError;
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

// ---- The disk format (decision row 61) ------------------------------------

#[test]
fn disk_round_trip_preserves_the_journal_exactly() {
    let mut engine = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    let mut journal = Journal::for_engine(&engine);
    for command in [
        deposit(ALICE, USD, 200_000_000_000),
        deposit(ALICE, BTC, 500_000_000),
        gtc_place(1, ALICE, Side::Bid, 1_000_000, 3),
        Command::Reduce {
            id: OrderId(1),
            by: lots(1),
        },
        Command::Move {
            id: OrderId(1),
            new_price: price(1_000_500),
        },
        Command::Cancel { id: OrderId(1) },
        Command::SetPositionLimit {
            user: ALICE,
            currency: USD,
            cap: 1_000_000_000,
        },
        Command::ClearPositionLimit {
            user: ALICE,
            currency: USD,
        },
    ] {
        journal.record(&mut engine, command);
    }
    let bytes = journal.to_bytes();
    let loaded = Journal::from_bytes(&bytes).expect("own encoding must decode");
    assert_eq!(
        loaded.header, journal.header,
        "header must ride in the file"
    );
    assert_eq!(
        loaded.entries, journal.entries,
        "every entry byte-identical"
    );
    // And the loaded journal replays identically.
    let replayed = loaded.replay().expect("valid journal must replay");
    assert_eq!(deep_digest(&engine), deep_digest(&replayed));
}

#[test]
fn file_round_trip_and_missing_file_are_honest() {
    let mut engine = Engine::new(SYMBOL, USD, BTC);
    let mut journal = Journal::for_engine(&engine);
    journal.record(&mut engine, deposit(ALICE, USD, 1_000_000_000));
    let path = std::env::temp_dir().join("launchpad-journal-test.lpdj");
    journal.to_file(&path).expect("temp write must succeed");
    let loaded = Journal::from_file(&path).expect("temp read must succeed");
    assert_eq!(loaded, journal);
    std::fs::remove_file(&path).expect("cleanup must succeed");
    match Journal::from_file(&path) {
        Err(JournalError::Io(kind)) => assert_eq!(kind, std::io::ErrorKind::NotFound),
        other => panic!("missing file must be Io(NotFound), got {other:?}"),
    }
}

#[test]
fn corrupt_streams_are_errors_not_panics() {
    let mut engine = Engine::new(SYMBOL, USD, BTC);
    let mut journal = Journal::for_engine(&engine);
    journal.record(&mut engine, deposit(ALICE, USD, 1_000_000_000));
    journal.record(&mut engine, gtc_place(1, ALICE, Side::Bid, 1_000_000, 2));
    let good = journal.to_bytes();

    // Bad magic.
    let mut bad = good.clone();
    bad[3] = b'X';
    assert_eq!(Journal::from_bytes(&bad), Err(JournalError::Corrupt));
    // Truncated mid-frame.
    for cut in [0, 5, 17, good.len() - 1] {
        assert_eq!(
            Journal::from_bytes(&good[..cut]),
            Err(JournalError::Corrupt),
            "truncation at {cut} must be corrupt, never a panic"
        );
    }
    // Trailing garbage.
    let mut bad = good.clone();
    bad.push(0);
    assert_eq!(Journal::from_bytes(&bad), Err(JournalError::Corrupt));
    // Corrupted entry payload (flip a byte inside the last frame).
    let mut bad = good.clone();
    let last = bad.len() - 3;
    bad[last] ^= 0xff;
    let decoded = Journal::from_bytes(&bad);
    // Either it decodes to something different (a changed field) or it is
    // rejected — both are fine; a panic or a silent success is not.
    if let Ok(loaded) = decoded {
        assert_ne!(
            loaded.entries, journal.entries,
            "flipped byte must change the meaning"
        );
    }
    // Unknown command tag.
    let mut payload = Vec::new();
    payload.extend_from_slice(&1u64.to_le_bytes()); // accepted
    payload.extend_from_slice(&999u64.to_le_bytes()); // tag
    let mut frame = 8u64.to_le_bytes().to_vec();
    frame.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    let mut bad = Vec::new();
    bad.extend_from_slice(&Journal::MAGIC);
    bad.extend_from_slice(&0u64.to_le_bytes()); // no header
    bad.extend_from_slice(&1u64.to_le_bytes()); // one entry
    bad.extend_from_slice(&frame);
    bad.extend_from_slice(&payload);
    assert_eq!(Journal::from_bytes(&bad), Err(JournalError::Corrupt));
    // Invalid header fee rate (> 10_000 bps) must be Corrupt, not a panic.
    let mut bad = Vec::new();
    bad.extend_from_slice(&Journal::MAGIC);
    bad.extend_from_slice(&1u64.to_le_bytes()); // header present
    bad.extend_from_slice(&1u64.to_le_bytes()); // symbol
    bad.extend_from_slice(&1u64.to_le_bytes()); // quote
    bad.extend_from_slice(&2u64.to_le_bytes()); // base
    bad.extend_from_slice(&20_000u64.to_le_bytes()); // maker bps: invalid
    bad.extend_from_slice(&25u64.to_le_bytes()); // taker bps
    bad.extend_from_slice(&0u64.to_le_bytes()); // no entries
    assert_eq!(Journal::from_bytes(&bad), Err(JournalError::Corrupt));
    // The empty journal round-trips.
    let empty = Journal::new();
    assert_eq!(Journal::from_bytes(&empty.to_bytes()), Ok(empty));
}

proptest! {
    /// The disk format on random streams: encode → decode → replay must
    /// equal the original journal's replay, entry for entry.
    #[test]
    fn disk_round_trip_on_random_commands(
        cmds in prop::collection::vec(cmd_strategy(), 0..=80),
    ) {
        let mut engine = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
        let mut journal = Journal::for_engine(&engine);
        for command in &cmds {
            journal.record(&mut engine, *command);
        }
        let loaded = Journal::from_bytes(&journal.to_bytes())
            .expect("own encoding of valid entries must decode");
        prop_assert_eq!(&loaded.header, &journal.header);
        prop_assert_eq!(&loaded.entries, &journal.entries);
        let replayed = loaded
            .clone()
            .replay()
            .expect("valid journal must replay");
        prop_assert_eq!(deep_digest(&engine), deep_digest(&replayed));
    }
}

// ---- Snapshots (decision row 61): restore must equal replay ---------------

#[test]
fn snapshot_restore_matches_full_replay_hand_scenario() {
    // A stream that ends with resting orders, dust-bearing bid locks, fee
    // sink entries, and a position cap — every restore dimension exercised.
    let commands = vec![
        deposit(ALICE, USD, 200_000_000_000),
        deposit(ALICE, BTC, 500_000_000),
        deposit(BOB, USD, 200_000_000_000),
        deposit(BOB, BTC, 500_000_000),
        gtc_place(1, ALICE, Side::Bid, 1_000_000, 3),
        gtc_place(2, BOB, Side::Ask, 1_001_000, 2),
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
        }, // partial fill of BOB's ask: fees + a dusty bid lock on the remainder
        Command::SetPositionLimit {
            user: BOB,
            currency: BTC,
            cap: 400_000_000,
        },
    ];
    let mut engine = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    let mut journal = Journal::for_engine(&engine);
    for command in &commands {
        journal.record(&mut engine, *command);
    }
    // Capture at the current position; prove restore == the original state.
    let snap = journal.snapshot_at(&engine);
    assert_eq!(snap.up_to, commands.len());
    let mut restored = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    restored
        .restore_snapshot(&snap.state)
        .expect("valid snapshot must restore");
    assert_eq!(
        deep_digest(&engine),
        deep_digest(&restored),
        "restore must reproduce the captured state exactly (incl. bid-lock dust)"
    );
    // And a restored engine keeps trading identically: a further command
    // must produce the same state on both.
    let extra = gtc_place(4, BOB, Side::Ask, 1_000_500, 1);
    journal.record(&mut engine, extra);
    let mut journal2 = Journal::for_engine(&restored);
    journal2.record(&mut restored, extra);
    assert_eq!(deep_digest(&engine), deep_digest(&restored));
}

#[test]
fn replay_from_snapshot_equals_full_replay() {
    let mut engine = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    let mut journal = Journal::for_engine(&engine);
    for command in [
        deposit(ALICE, USD, 200_000_000_000),
        deposit(ALICE, BTC, 500_000_000),
        deposit(BOB, USD, 200_000_000_000),
        deposit(BOB, BTC, 500_000_000),
        gtc_place(1, ALICE, Side::Bid, 1_000_000, 3),
        gtc_place(2, BOB, Side::Ask, 1_001_000, 2),
    ] {
        journal.record(&mut engine, command);
    }
    let snap = journal.snapshot_at(&engine);
    // Extend the journal past the snapshot point.
    for command in [
        Command::Cancel { id: OrderId(1) },
        gtc_place(3, ALICE, Side::Bid, 999_000, 2),
        Command::Reduce {
            id: OrderId(2),
            by: lots(1),
        },
    ] {
        journal.record(&mut engine, command);
    }
    let full = journal.replay().expect("valid journal must replay");

    let mut target = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    journal
        .replay_from(&mut target, &snap)
        .expect("composed recovery must succeed");
    assert_eq!(
        deep_digest(&full),
        deep_digest(&target),
        "restore + tail replay == full replay — the composed-recovery proof"
    );
}

#[test]
fn restore_rejects_inconsistent_and_misapplied_snapshots() {
    let mut engine = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    let mut journal = Journal::for_engine(&engine);
    for command in [
        deposit(ALICE, USD, 200_000_000_000),
        deposit(ALICE, BTC, 500_000_000),
        gtc_place(1, ALICE, Side::Bid, 1_000_000, 3),
    ] {
        journal.record(&mut engine, command);
    }
    let snap = journal.snapshot_at(&engine);

    // Non-fresh target.
    let mut dirty = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    journal.record(&mut dirty, deposit(BOB, USD, 1));
    assert!(matches!(
        dirty.restore_snapshot(&snap.state),
        Err(EngineError::BookNotEmpty)
    ));
    // Wrong header.
    let mut other = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(50, 60).unwrap());
    assert!(matches!(
        journal.replay_from(&mut other, &snap),
        Err(JournalError::HeaderMismatch { .. })
    ));

    // Internally inconsistent: account lock edited apart from Σ order locks.
    let mut inconsistent = snap.state.clone();
    inconsistent.accounts[0].3 += 1;
    let mut fresh = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    assert!(matches!(
        fresh.restore_snapshot(&inconsistent),
        Err(EngineError::Risk(RiskError::SnapshotLockMismatch { .. }))
    ));

    // A crossed snapshot is refused.
    let mut crossed = snap.state.clone();
    crossed.orders.push(SnapshotOrder {
        id: OrderId(9),
        user: BOB,
        symbol: SYMBOL,
        side: Side::Ask,
        price: Some(price(999_999)), // below the resting bid
        remaining: lots(1),
        currency: BTC,
        locked: 50_000_000,
    });
    let mut fresh = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    assert!(matches!(
        fresh.restore_snapshot(&crossed),
        Err(EngineError::Book(BookError::CrossedBook { .. }))
    ));

    // A resting row without a price is refused.
    let mut priceless = snap.state.clone();
    priceless.orders[0].price = None;
    let mut fresh = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    assert!(matches!(
        fresh.restore_snapshot(&priceless),
        Err(EngineError::Book(BookError::NotResting { .. }))
    ));

    // A cap below existing locks is a LEGAL state (row 54 freeze semantics):
    // restore must accept it and the gate re-arms on the next commit.
    let mut capped = snap.state.clone();
    capped.position_limits.push((ALICE, USD, 1)); // lock far exceeds cap 1
    let mut fresh = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
    fresh
        .restore_snapshot(&capped)
        .expect("cap-below-lock is legitimate frozen state, not corruption");
    // The re-armed gate bites: a new commit past cap 1 is refused.
    assert!(matches!(
        fresh.ledger_mut().commit(ALICE, USD, 2),
        Err(RiskError::PositionLimitExceeded { .. })
    ));
}

proptest! {
    /// The composed-recovery proof on random streams: restore at a random
    /// prefix point + tail replay == full replay, byte for byte.
    #[test]
    fn replay_from_random_snapshot_equals_full_replay(
        cmds in prop::collection::vec(cmd_strategy(), 1..=60),
        cut in 0usize..60,
    ) {
        let mut engine = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
        let mut journal = Journal::for_engine(&engine);
        let cut = cut.min(cmds.len() - 1);
        for (i, command) in cmds.iter().enumerate() {
            journal.record(&mut engine, *command);
            if i == cut {
                // Capture right after the cut-th command.
                let snap = journal.snapshot_at(&engine);
                let full = {
                    // Full replay of everything so far, for the prefix check.
                    journal.replay().expect("valid journal must replay")
                };
                let mut target = Engine::with_fees(SYMBOL, USD, BTC, FeeSchedule::new(10, 25).unwrap());
                journal
                    .replay_from(&mut target, &snap)
                    .expect("composed recovery must succeed");
                prop_assert_eq!(deep_digest(&full), deep_digest(&target));
            }
        }
    }
}
