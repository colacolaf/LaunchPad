use super::*;
use crate::domain::Price;

const USD: CurrencyId = CurrencyId(1);
const BTC: CurrencyId = CurrencyId(2);
const ALICE: UserId = UserId(1);
const BOB: UserId = UserId(2);

fn ledger_with(alice_usd: u64, alice_btc: u64, bob_usd: u64, bob_btc: u64) -> Ledger {
    let mut ledger = Ledger::new();
    for (user, currency, amount) in [
        (ALICE, USD, alice_usd),
        (ALICE, BTC, alice_btc),
        (BOB, USD, bob_usd),
        (BOB, BTC, bob_btc),
    ] {
        if amount > 0 {
            ledger.deposit(user, currency, amount).unwrap();
        }
    }
    ledger
}

// ---- read path -------------------------------------------------------

#[test]
fn unknown_pairs_read_as_zero() {
    let ledger = Ledger::new();
    assert_eq!(ledger.free(ALICE, USD), 0);
    assert_eq!(ledger.locked(ALICE, USD), 0);
}

#[test]
fn deposit_creates_and_accumulates() {
    let mut ledger = Ledger::new();
    ledger.deposit(ALICE, USD, 1_000).unwrap();
    ledger.deposit(ALICE, USD, 500).unwrap();
    assert_eq!(ledger.free(ALICE, USD), 1_500);
    assert_eq!(ledger.locked(ALICE, USD), 0);
}

// ---- deposit/withdraw rejections -------------------------------------

#[test]
fn zero_adjustments_are_rejected_not_ignored() {
    let mut ledger = Ledger::new();
    assert_eq!(
        ledger.deposit(ALICE, USD, 0),
        Err(RiskError::ZeroAmount { currency: USD })
    );
    assert_eq!(
        ledger.withdraw(ALICE, USD, 0),
        Err(RiskError::ZeroAmount { currency: USD })
    );
    // And nothing was created by the failed attempts:
    assert_eq!(ledger.free(ALICE, USD), 0);
}

#[test]
fn withdraw_more_than_free_is_rejected_untouched() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    assert_eq!(
        ledger.withdraw(ALICE, USD, 1_001),
        Err(RiskError::InsufficientFree {
            currency: USD,
            requested: 1_001,
            available: 1_000,
        })
    );
    assert_eq!(
        ledger.free(ALICE, USD),
        1_000,
        "account unchanged on reject"
    );
}

#[test]
fn withdraw_cannot_touch_locked_funds() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.commit(ALICE, USD, 600).unwrap();
    // 400 free, 600 locked. A withdraw of 500 must fail even though the
    // account "holds" 1_000 — locked funds belong to resting orders.
    assert!(matches!(
        ledger.withdraw(ALICE, USD, 500),
        Err(RiskError::InsufficientFree { .. })
    ));
    assert_eq!(ledger.free(ALICE, USD), 400);
    assert_eq!(ledger.locked(ALICE, USD), 600);
}

// ---- commit (the place-time lock) -------------------------------------

#[test]
fn commit_moves_free_to_locked() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.commit(ALICE, USD, 400).unwrap();
    assert_eq!(ledger.free(ALICE, USD), 600);
    assert_eq!(ledger.locked(ALICE, USD), 400);
}

#[test]
fn commit_beyond_free_is_rejected_all_or_nothing() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    assert_eq!(
        ledger.commit(ALICE, USD, 1_001),
        Err(RiskError::InsufficientForOrder {
            currency: USD,
            required: 1_001,
            available: 1_000,
        })
    );
    // All-or-nothing: not even a partial lock happened.
    assert_eq!(ledger.free(ALICE, USD), 1_000);
    assert_eq!(ledger.locked(ALICE, USD), 0);
}

#[test]
fn second_bid_cannot_double_spend_the_same_free_balance() {
    // THE over-commitment case that decided reserve-vs-check: deposit
    // 100 once, place two bids that each need 100. The second must
    // reject — a check-only ledger would have accepted both.
    let mut ledger = ledger_with(100, 0, 0, 0);
    ledger.commit(ALICE, USD, 100).unwrap();
    assert!(matches!(
        ledger.commit(ALICE, USD, 100),
        Err(RiskError::InsufficientForOrder { .. })
    ));
    assert_eq!(ledger.free(ALICE, USD), 0);
    assert_eq!(ledger.locked(ALICE, USD), 100);
}

#[test]
fn ask_commits_base_lots_bid_commits_quote_ticks() {
    // Same ledger API, different currency per side — the *engine* decides
    // which currency an order draws on; the ledger just moves integers.
    let mut ledger = ledger_with(5_000_000, 3, 0, 0);
    ledger.commit(ALICE, BTC, 2).unwrap(); // ask: lock 2 base lots
    assert_eq!(ledger.free(ALICE, BTC), 1);
    ledger.commit(ALICE, USD, 5_000_000).unwrap(); // bid: lock quote
    assert_eq!(ledger.free(ALICE, USD), 0);
}

// ---- release (cancel / unused remainder) ------------------------------

#[test]
fn release_returns_exactly_the_locked_amount() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.commit(ALICE, USD, 400).unwrap();
    ledger.release(ALICE, USD, 400);
    assert_eq!(ledger.free(ALICE, USD), 1_000);
    assert_eq!(ledger.locked(ALICE, USD), 0);
}

#[test]
fn partial_fill_releases_the_unused_remainder_of_the_lock() {
    // Locked 400; a fill consumes 250 of it; the other 150 returns.
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.commit(ALICE, USD, 400).unwrap();
    ledger.settle(ALICE, BOB, USD, 250);
    assert_eq!(ledger.locked(ALICE, USD), 150);
    ledger.release(ALICE, USD, 150);
    assert_eq!(ledger.free(ALICE, USD), 750);
    assert_eq!(ledger.locked(ALICE, USD), 0);
}

// ---- settle (fills move value between users) ---------------------------

#[test]
fn settle_moves_value_payer_to_payee_only() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.commit(ALICE, USD, 400).unwrap();
    ledger.settle(ALICE, BOB, USD, 250);
    // Payer's lock shrinks by exactly the fill; payee gains free funds.
    assert_eq!(ledger.locked(ALICE, USD), 150);
    assert_eq!(ledger.free(BOB, USD), 250);
    // Conservation inside the ledger: 750 + 150 + 250 == 1_000.
    assert_eq!(
        ledger.free(ALICE, USD) + ledger.locked(ALICE, USD) + ledger.free(BOB, USD),
        1_000
    );
}

#[test]
fn self_trade_returns_lock_to_free_without_transfer() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.commit(ALICE, USD, 400).unwrap();
    ledger.settle(ALICE, ALICE, USD, 250);
    assert_eq!(ledger.locked(ALICE, USD), 150);
    assert_eq!(ledger.free(ALICE, USD), 850);
    // Total unchanged: 850 + 150 == 1_000.
}

// ---- the ceil-rounding helper ------------------------------------------

#[test]
fn quote_cost_is_exact_when_divisible() {
    // 1.0 base = QTY_SCALE lots at 150.0 quote (1_500_000 ticks):
    // cost = 100_000_000 × 1_500_000 / 100_000_000 = 1_500_000 ticks exact.
    let qty = Qty::from_lots(100_000_000).unwrap(); // exactly 1.0 base
    let price = Price::from_ticks(1_500_000).unwrap(); // exactly 150.0 quote
    assert_eq!(quote_cost_ticks(qty, price), Some(1_500_000));
}

#[test]
fn quote_cost_ceils_never_floors() {
    // 1 lot (1e-8 base) at 1 tick: exact cost is 1e-8 ticks — rounds UP
    // to 1 tick. Under-charging (0) would let a lock cover a fill it
    // cannot pay for. (First draft of this test expected "30_000" here —
    // the tutor conflated lots with base units; the property test below
    // is what caught it. Lots are 1e-8 base: order sizes in lots are
    // huge numbers, costs in ticks stay readable.)
    let qty = Qty::from_lots(1).unwrap();
    let price = Price::from_ticks(1).unwrap();
    assert_eq!(quote_cost_ticks(qty, price), Some(1));
}

#[test]
fn quote_cost_overflow_is_none_not_wrap() {
    let qty = Qty::from_lots(u64::MAX).unwrap();
    let price = Price::from_ticks(u64::MAX).unwrap();
    assert_eq!(quote_cost_ticks(qty, price), None);
}

#[test]
fn quote_cost_realistic_order_with_remainder() {
    // 1.23456789 base at 98765.4321 quote:
    // 123_456_789 lots × 987_654_321 ticks = 121_932_631_112_635_269,
    // / 1e8 = 1_219_326_311.1263… → ceil → 1_219_326_312 ticks.
    let qty = Qty::from_lots(123_456_789).unwrap();
    let price = Price::from_ticks(987_654_321).unwrap();
    assert_eq!(quote_cost_ticks(qty, price), Some(1_219_326_312));
}

// ---- error display smoke ------------------------------------------------

#[test]
fn errors_render_readably() {
    let err = RiskError::InsufficientForOrder {
        currency: USD,
        required: 10,
        available: 4,
    };
    let msg = err.to_string();
    assert!(msg.contains("10") && msg.contains("4"));
    assert!(matches!(
        RiskError::ZeroAmount { currency: BTC },
        RiskError::ZeroAmount { .. }
    ));
}

// ---- Property tests ----------------------------------------------------

use proptest::prelude::*;

/// One randomized ledger operation. Deposit/withdraw exercise the
/// external funding commands; Commit/Release/Settle exercise the
/// order lifecycle the Phase 1 engine will drive.
#[derive(Debug, Clone, Copy)]
enum LCmd {
    Deposit { pick: u64, amount: u64 },
    Withdraw { pick: u64, amount: u64 },
    Commit { pick: u64, amount: u64 },
    Release { pick: u64, amount: u64 },
    Settle { from: u64, to: u64, amount: u64 },
}

fn lcmd_strategy() -> impl Strategy<Value = LCmd> {
    // Small user space: collisions between users are frequent, which is
    // where transfer bugs live. Amounts kept small: rejections stay
    // common, which is itself a path we want exercised.
    let user = 0u64..4;
    let amount = 1u64..50;
    prop_oneof![
        2 => (user.clone(), amount.clone()).prop_map(|(pick, amount)| LCmd::Deposit { pick, amount }),
        1 => (user.clone(), amount.clone()).prop_map(|(pick, amount)| LCmd::Withdraw { pick, amount }),
        3 => (user.clone(), amount.clone()).prop_map(|(pick, amount)| LCmd::Commit { pick, amount }),
        2 => (user.clone(), amount.clone()).prop_map(|(pick, amount)| LCmd::Release { pick, amount }),
        2 => (user.clone(), user, amount).prop_map(|(from, to, amount)| LCmd::Settle { from, to, amount }),
    ]
}

/// Drive a randomized command sequence through the ledger with a
/// model that mirrors the intended semantics, checking after every op:
///
/// 1. **Conservation** — total (free + locked) across users changes only
///    by deposit (+) and withdraw (−). Every commit/release/settle is
///    internal reshuffling.
/// 2. **No overdraft** — free ≥ 0 and locked ≥ 0 for every account,
///    always (u64 makes negatives unrepresentable; the property asserts
///    the *model* and the ledger agree, which catches semantic drift).
///
/// The model tracks free/locked per user with plain arithmetic and
/// skips ops the real ledger would reject — except conservation
/// violations, which must never happen and would fail loudly here.
fn run_ledger(cmds: &[LCmd]) {
    use crate::domain::{CurrencyId, UserId};
    let cur = CurrencyId(7);
    let users: Vec<UserId> = (1..=4).map(UserId).collect();
    let uid = |pick: u64| users[(pick % 4) as usize];

    let mut ledger = Ledger::new();
    // Model: (free, locked) per user. Deposits are the only external in,
    // withdrawals the only external out.
    let mut model: Vec<(u64, u64)> = vec![(0, 0); 4];
    let mut total_deposited: u64 = 0;

    for cmd in cmds {
        match *cmd {
            LCmd::Deposit { pick, amount } => {
                ledger.deposit(uid(pick), cur, amount).unwrap();
                model[(pick % 4) as usize].0 += amount;
                total_deposited += amount;
            }
            LCmd::Withdraw { pick, amount } => {
                let idx = (pick % 4) as usize;
                if model[idx].0 >= amount {
                    ledger.withdraw(uid(pick), cur, amount).unwrap();
                    model[idx].0 -= amount;
                    total_deposited -= amount;
                } else {
                    assert!(matches!(
                        ledger.withdraw(uid(pick), cur, amount),
                        Err(RiskError::InsufficientFree { .. })
                    ));
                }
            }
            LCmd::Commit { pick, amount } => {
                let idx = (pick % 4) as usize;
                if model[idx].0 >= amount {
                    ledger.commit(uid(pick), cur, amount).unwrap();
                    model[idx].0 -= amount;
                    model[idx].1 += amount;
                } else {
                    assert!(matches!(
                        ledger.commit(uid(pick), cur, amount),
                        Err(RiskError::InsufficientForOrder { .. })
                    ));
                }
            }
            LCmd::Release { pick, amount } => {
                let idx = (pick % 4) as usize;
                // Only release what the model says is locked — a release
                // beyond the lock is an engine bug the ledger may panic
                // on, and the model exists to prevent driving it there.
                let capped = amount.min(model[idx].1);
                if capped > 0 {
                    ledger.release(uid(pick), cur, capped);
                    model[idx].1 -= capped;
                    model[idx].0 += capped;
                }
            }
            LCmd::Settle { from, to, amount } => {
                let fi = (from % 4) as usize;
                // Settle draws from the payer's lock only.
                let capped = amount.min(model[fi].1);
                if capped > 0 {
                    let ti = (to % 4) as usize;
                    ledger.settle(uid(from), uid(to), cur, capped);
                    model[fi].1 -= capped;
                    if fi != ti {
                        model[ti].0 += capped;
                    } else {
                        model[fi].0 += capped;
                    }
                }
            }
        }

        // Conservation, after every operation: model total == deposited.
        let model_total: u64 = model.iter().map(|(f, l)| f + l).sum();
        assert_eq!(
            model_total, total_deposited,
            "model diverged from conservation after {cmd:?}"
        );
        // Ledger agrees with the model, account by account.
        for (idx, user) in users.iter().enumerate() {
            assert_eq!(
                (ledger.free(*user, cur), ledger.locked(*user, cur)),
                model[idx],
                "ledger/model divergence for user {user:?} after {cmd:?}"
            );
        }
    }
}

// ---- Fees: the schedule -------------------------------------------------

#[test]
fn zero_schedule_is_the_no_fee_behavior() {
    let schedule = FeeSchedule::zero();
    assert_eq!(schedule.maker_fee(0), 0);
    assert_eq!(schedule.taker_fee(1), 0);
    assert_eq!(schedule.maker_fee(987_654_321), 0);
}

#[test]
fn fee_rates_validate_at_the_boundary() {
    assert!(FeeSchedule::new(0, 0).is_ok());
    assert!(FeeSchedule::new(10_000, 10_000).is_ok(), "100% is legal");
    assert_eq!(
        FeeSchedule::new(10_001, 0),
        Err(FeeError::RateTooHigh {
            maker_bps: 10_001,
            taker_bps: 0,
        })
    );
    assert_eq!(
        FeeSchedule::new(0, u64::MAX),
        Err(FeeError::RateTooHigh {
            maker_bps: 0,
            taker_bps: u64::MAX,
        })
    );
}

#[test]
fn fee_math_floors_on_dust() {
    let schedule = FeeSchedule::new(10, 20).unwrap(); // 0.10% / 0.20%
    // 15_000 ticks × 10 bps = 15 exactly.
    assert_eq!(schedule.maker_fee(15_000), 15);
    // 12_345 × 20 bps = 24.69 → floor 24: the exchange under-collects
    // on dust, never over-charges.
    assert_eq!(schedule.taker_fee(12_345), 24);
    // A 1-tick fill pays nothing at any sane rate.
    assert_eq!(schedule.maker_fee(1), 0);
    assert_eq!(schedule.taker_fee(9_999), 19, "floor of 19.998");
}

#[test]
fn fee_never_exceeds_the_received_value_even_at_100_percent() {
    let schedule = FeeSchedule::new(10_000, 10_000).unwrap();
    assert_eq!(schedule.maker_fee(7), 7);
    assert_eq!(schedule.taker_fee(u64::MAX), u64::MAX);
}

#[test]
fn fee_computation_does_not_overflow_on_huge_receipts() {
    // Naive `received × bps` overflows for received > ~1.8e15; the
    // split-multiply path must return the exact floor instead.
    let schedule = FeeSchedule::new(10, 10).unwrap();
    let received = u64::MAX; // ~1.8e19 — far into naive-overflow range
    // floor((2^64 − 1) × 10 / 10_000) = 18_446_744_073_709_551.
    assert_eq!(schedule.maker_fee(received), 18_446_744_073_709_551);
}

// ---- Fees: the sink ------------------------------------------------------

#[test]
fn fees_collected_reads_zero_for_unknown_currency() {
    let ledger = Ledger::new();
    assert_eq!(ledger.fees_collected(USD), 0);
}

#[test]
fn collect_fee_moves_free_to_the_sink() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.collect_fee(ALICE, USD, 40);
    assert_eq!(ledger.free(ALICE, USD), 960);
    assert_eq!(ledger.fees_collected(USD), 40);
    assert_eq!(ledger.locked(ALICE, USD), 0);
}

#[test]
fn zero_fee_collection_is_a_no_op() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.collect_fee(ALICE, USD, 0);
    assert_eq!(ledger.free(ALICE, USD), 1_000);
    assert_eq!(ledger.fees_collected(USD), 0);
}

#[test]
fn fee_sink_accumulates_across_collections_and_currencies() {
    let mut ledger = ledger_with(1_000, 500, 1_000, 0);
    ledger.collect_fee(ALICE, USD, 10);
    ledger.collect_fee(BOB, USD, 3);
    ledger.collect_fee(ALICE, BTC, 7);
    assert_eq!(ledger.fees_collected(USD), 13);
    assert_eq!(ledger.fees_collected(BTC), 7);
    // Conservation with fees: Σ(free+locked) + Σ(fees) = deposits.
    assert_eq!(
        ledger.free(ALICE, USD)
            + ledger.locked(ALICE, USD)
            + ledger.free(BOB, USD)
            + ledger.fees_collected(USD),
        2_000
    );
    assert_eq!(
        ledger.free(ALICE, BTC) + ledger.locked(ALICE, BTC) + ledger.fees_collected(BTC),
        500
    );
}

#[test]
#[should_panic]
fn collect_fee_beyond_free_panics_engine_bug_not_input() {
    let mut ledger = ledger_with(10, 0, 0, 0);
    ledger.collect_fee(ALICE, USD, 11);
}

// ---- Position limits (Phase 3, decision row 54) -------------------------

#[test]
fn commit_at_the_cap_passes_exactly() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.set_position_limit(ALICE, USD, 400);
    ledger.commit(ALICE, USD, 400).unwrap(); // exactly at the cap — allowed
    assert_eq!(ledger.locked(ALICE, USD), 400);
}

#[test]
fn commit_past_the_cap_is_rejected_untouched() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.set_position_limit(ALICE, USD, 300);
    assert!(matches!(
        ledger.commit(ALICE, USD, 301),
        Err(RiskError::PositionLimitExceeded {
            cap: 300,
            would_lock: 301,
            ..
        })
    ));
    // Untouched: free and locked exactly as before the rejection.
    assert_eq!(ledger.free(ALICE, USD), 1_000);
    assert_eq!(ledger.locked(ALICE, USD), 0);
}

#[test]
fn cap_counts_locks_that_predate_it_and_grows_to_the_cap() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.commit(ALICE, USD, 200).unwrap(); // locked BEFORE the cap exists
    ledger.set_position_limit(ALICE, USD, 500);
    ledger.commit(ALICE, USD, 300).unwrap(); // 200 + 300 = 500 = cap: ok
    assert_eq!(ledger.locked(ALICE, USD), 500);
    assert!(matches!(
        ledger.commit(ALICE, USD, 1),
        Err(RiskError::PositionLimitExceeded {
            cap: 500,
            would_lock: 501,
            ..
        })
    ));
}

#[test]
fn cap_below_current_lock_freezes_growth_but_releases_still_work() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.commit(ALICE, USD, 400).unwrap();
    ledger.set_position_limit(ALICE, USD, 100); // below the 400 locked: freeze
    assert!(ledger.commit(ALICE, USD, 1).is_err());
    ledger.release(ALICE, USD, 400); // releases are unaffected by the cap
    assert_eq!(ledger.locked(ALICE, USD), 0);
    ledger.commit(ALICE, USD, 100).unwrap(); // growth allowed again under cap
    assert_eq!(ledger.locked(ALICE, USD), 100);
}

#[test]
fn zero_cap_is_a_full_freeze() {
    let mut ledger = ledger_with(1_000, 0, 0, 0);
    ledger.set_position_limit(ALICE, USD, 0);
    assert!(matches!(
        ledger.commit(ALICE, USD, 1),
        Err(RiskError::PositionLimitExceeded {
            cap: 0,
            would_lock: 1,
            ..
        })
    ));
    assert_eq!(ledger.locked(ALICE, USD), 0);
}

#[test]
fn limits_are_per_account_per_currency_and_clearable() {
    let mut ledger = ledger_with(1_000, 500, 1_000, 0);
    ledger.set_position_limit(ALICE, USD, 100);
    // BOB is uncapped; ALICE's other currency is uncapped.
    ledger.commit(BOB, USD, 500).unwrap();
    ledger.commit(ALICE, BTC, 400).unwrap();
    assert!(ledger.commit(ALICE, USD, 101).is_err());
    ledger.clear_position_limit(ALICE, USD);
    ledger.commit(ALICE, USD, 101).unwrap(); // uncapped again
    assert_eq!(ledger.position_limit(ALICE, USD), None);
}

#[test]
fn clearing_an_absent_limit_is_a_no_op() {
    let mut ledger = Ledger::new();
    ledger.clear_position_limit(ALICE, USD); // must not panic
    assert_eq!(ledger.position_limit(ALICE, USD), None);
}

#[test]
fn position_limit_reads_none_when_unset() {
    let ledger = ledger_with(1_000, 0, 0, 0);
    assert_eq!(ledger.position_limit(ALICE, USD), None);
}

proptest! {
    /// Conservation + model agreement under randomized op sequences.
    #[test]
    fn ledger_conserves_total_under_random_ops(
        cmds in prop::collection::vec(lcmd_strategy(), 0..=80),
    ) {
        run_ledger(&cmds);
    }
}

proptest! {
    /// The fee term in conservation: random fee collections off a
    /// randomized funded ledger never mint or destroy value — every
    /// collected unit is accounted for in the sink, and the sink total
    /// is exactly Σ collections.
    #[test]
    fn fee_collections_conserve_with_sink_term(
        (funding, collections) in (prop::collection::vec(1u64..200, 1..=4),
            prop::collection::vec((0u64..4, 0u64..30), 0..=40)),
    ) {
        use crate::domain::{CurrencyId, UserId};
        let cur = CurrencyId(9);
        let users: Vec<UserId> = (1..=4).map(UserId).collect();
        let mut ledger = Ledger::new();
        let mut deposited: u64 = 0;
        for (idx, &amount) in funding.iter().enumerate() {
            ledger.deposit(users[idx], cur, amount).unwrap();
            deposited += amount;
        }
        let mut collected: u64 = 0;
        for (pick, amount) in collections {
            let user = users[(pick as usize) % users.len()];
            // Mirror's free check: only collect when the model says it
            // fits (the ledger itself panics on a short free).
            if ledger.free(user, cur) >= amount {
                ledger.collect_fee(user, cur, amount);
                collected += amount;
            }
        }
        let balances: u64 = users
            .iter()
            .map(|&u| ledger.free(u, cur) + ledger.locked(u, cur))
            .sum();
        assert_eq!(balances + ledger.fees_collected(cur), deposited);
        assert_eq!(ledger.fees_collected(cur), collected);
    }
}
