//! Latency benchmark — Phase 2 baseline v1 (`docs/benchmarks.md`).
//!
//! A `harness = false` binary: no criterion, no sampling — every op is
//! timed and every sample lands in the table. Exact percentiles
//! (p50/p90/p99/p99.9/max) over the full sorted sample set, plus the
//! audit counters (mix realized, trades, cancel-miss rate) that make the
//! run itself falsifiable, the two-run determinism digest, and the
//! conservation check. Same seed → same commands → same digest; a
//! mismatch fails the run instead of being printed away.
//!
//! `LAUNCHPAD_BENCH_OPS` scales the command count (CI smoke uses a small
//! value); percentiles stay exact at any size.

mod support;

use support::{Counters, Workload, assert_conservation, percentile, state_digest};

/// Default command count for a full run (override: `LAUNCHPAD_BENCH_OPS`).
const DEFAULT_OPS: u64 = 50_000;

/// Run the seeded stream for `ops` commands, timing every op.
fn run_stream(seed: u64, ops: u64) -> (Vec<u64>, Counters, u64) {
    let mut workload = Workload::new(seed);
    let mut engine = Workload::fresh_engine();
    workload.bootstrap(&mut engine);

    let mut samples: Vec<u64> = Vec::with_capacity(ops as usize);
    for _ in 0..ops {
        let timer = support::OpTimer::start();
        workload.step(&mut engine);
        samples.push(timer.elapsed_nanos());
    }

    workload.assert_mirror_matches_engine(&engine);
    assert_conservation(&engine);
    let digest = state_digest(&engine);
    (samples, workload.counters(), digest)
}

/// Print the exact percentile table + counters. One block, grep-able, in
/// the shape `docs/benchmarks.md` prescribes.
fn report(title: &str, samples: &[u64], counters: Counters, ops: u64) {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let total: u64 = sorted.iter().sum();
    let mean = total / sorted.len() as u64;

    println!("\n== {title} ==");
    println!("ops: {ops}  seed: fixed (see source)");
    println!(
        "mix realized: gtc={} ioc={} cancel_ok={} cancel_missed={} moves_ok={} moves_rejected={}",
        counters.gtc_places,
        counters.ioc_places,
        counters.cancels_ok,
        counters.cancels_missed,
        counters.moves_ok,
        counters.moves_rejected
    );
    println!(
        "trades: {}  volume: {} lots  ({} fills/op)",
        counters.trades,
        counters.volume_lots,
        counters.trades as f64 / ops as f64
    );
    println!("mean: {mean} ns/op");
    for p in [50.0, 90.0, 99.0, 99.9] {
        println!("p{p:>4}: {:>8} ns", percentile(&sorted, p));
    }
    println!("max:  {:>8} ns", sorted[sorted.len() - 1]);
}

fn main() {
    let ops: u64 = std::env::var("LAUNCHPAD_BENCH_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_OPS);

    // Two full runs of the same seeded stream: identical digests are the
    // bench-scale replay-determinism proof. Divergence = loud failure.
    let (samples_a, counters_a, digest_a) = run_stream(0xFEED, ops);
    let (samples_b, counters_b, digest_b) = run_stream(0xFEED, ops);

    assert_eq!(
        digest_a, digest_b,
        "two runs of the same seeded stream diverged — determinism broken"
    );
    assert_eq!(
        counters_a.trades, counters_b.trades,
        "trade-count divergence"
    );

    report(
        "latency: reference mix (single-symbol engine)",
        &samples_a,
        counters_a,
        ops,
    );
    report(
        "latency: run 2 (digest match proof)",
        &samples_b,
        counters_b,
        ops,
    );
    println!("\ndeterminism: digests match ({digest_a:#018x})");
}
