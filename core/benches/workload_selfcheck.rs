//! Workload self-check — the generator's own contract, run as a real
//! target (the `#[test]` form never executed: support is a `harness =
//! false` bench dependency, so cargo's test harness never sees it — a
//! dead test is a lie, so these checks are a proper bench target that CI
//! runs). Exit 0 = the seeded stream is reproducible and the mix
//! exercises every command kind; any violation panics loudly.

mod support;

use support::{Workload, assert_conservation, state_digest};

fn main() {
    // Contract 1: two runs of the same seed build identical states.
    let mut a = Workload::new(42);
    let mut engine_a = Workload::fresh_engine();
    a.bootstrap(&mut engine_a);
    for _ in 0..2_000 {
        a.step(&mut engine_a);
    }

    let mut b = Workload::new(42);
    let mut engine_b = Workload::fresh_engine();
    b.bootstrap(&mut engine_b);
    for _ in 0..2_000 {
        b.step(&mut engine_b);
    }

    a.assert_mirror_matches_engine(&engine_a);
    b.assert_mirror_matches_engine(&engine_b);
    assert_eq!(
        state_digest(&engine_a),
        state_digest(&engine_b),
        "same seed, different final state — generator is not deterministic"
    );
    assert_eq!(
        a.counters().trades,
        b.counters().trades,
        "trade-count divergence between identical seeds"
    );

    // Contract 2: the mix actually produces all four command kinds and a
    // nonzero trade rate — a silent zero-trade run would pass every
    // percentile check while measuring nothing of interest.
    let mut workload = Workload::new(7);
    let mut engine = Workload::fresh_engine();
    workload.bootstrap(&mut engine);
    for _ in 0..5_000 {
        workload.step(&mut engine);
    }
    let counters = workload.counters();
    assert!(counters.gtc_places > 100, "gtc: {counters:?}");
    assert!(counters.ioc_places > 30, "ioc: {counters:?}");
    assert!(counters.cancels_ok > 60, "cancels: {counters:?}");
    assert!(counters.moves_ok > 1_000, "moves: {counters:?}");
    assert!(counters.trades > 0, "no trades at all: {counters:?}");
    assert!(counters.volume_lots > 0, "no volume: {counters:?}");
    workload.assert_mirror_matches_engine(&engine);
    assert_conservation(&engine);

    println!("workload self-check OK (reproducible, all four kinds, conservation holds)");
}
