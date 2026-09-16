//! Throughput benchmark — Phase 2 baseline v1 (`docs/benchmarks.md`).
//!
//! Reports ops/sec through criterion for the reference mix and for each
//! operation kind in isolation. All numbers land in the same methodology
//! section as the latency table: same hardware, same seed, same release
//! profile (`lto = fat`, `codegen-units = 1`), same disclosure.
//!
//! Scope, disclosed: each op includes the generator's bookkeeping (mirror
//! update, counter tick) on top of the engine call — the full client-to-ack
//! path, not the bare engine call. The gap is the methodology's business,
//! not something to hide.
//!
//! Every measurement group bootstraps its own steady state per iteration
//! (same seed → same book shape), so batches are comparable instead of
//! drifting deeper into a random walk. `Throughput::Elements` makes
//! criterion report per-element (per-op) cost alongside the raw rate.

mod support;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use support::{PRELOAD_ORDERS, Workload};

/// Commands per measured batch. 50k so the per-iteration 1,000-order
/// bootstrap is ~2% of the measured work (disclosed scope: every batch
/// re-bootstraps its steady state — the number is for per-op cost, and the
/// bootstrap share is stated here rather than hidden).
const OPS_PER_GROUP: u64 = 50_000;

/// One mix arm: `fn(&mut Workload, &mut Engine)` — throughput's per-op
/// chunks drive exactly one arm each via this handle.
type ArmDriver = fn(&mut Workload, &mut launchpad_core::engine::Engine);

/// Run `ops` commands driven exclusively by `driver` (one mix arm), then
/// verify the mirror and return the refill count.
///
/// Draining arms get a disclosed refill rule: cancel consumes live orders,
/// IOC fills kill makers, so a pure-drain chunk would collapse to an
/// empty-book degenerate (the first local run measured 4.7 ns/cancel — a
/// number measuring nothing). Refill = place GTCs when the book falls
/// below `refill_below`. In steady state inflow = outflow (cancel: ~50%
/// of ops are refills; IOC: ~2.5%), which is what any real book needs —
/// the per-op cost is recoverable from the published numbers, and the
/// refill count is black-boxed so the placement work cannot be optimized
/// away.
fn run_arm(
    (mut workload, mut engine): (Workload, launchpad_core::engine::Engine),
    ops: u64,
    driver: ArmDriver,
    refill_below: usize,
) -> u64 {
    let mut refills: u64 = 0;
    for _ in 0..ops {
        if workload.live_len() < refill_below {
            workload.step_gtc(&mut engine);
            refills += 1;
        }
        driver(&mut workload, &mut engine);
    }
    workload.assert_mirror_matches_engine(&engine);
    black_box(&engine);
    black_box(refills)
}

fn bench_reference_mix(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("engine/reference_mix");
    group.throughput(Throughput::Elements(OPS_PER_GROUP));
    group.bench_function("mixed 9/3/6/82", |b| {
        b.iter_batched(
            || {
                let mut workload = Workload::new(11);
                let mut engine = Workload::fresh_engine();
                workload.bootstrap(&mut engine);
                (workload, engine)
            },
            |(mut workload, mut engine)| {
                for _ in 0..OPS_PER_GROUP {
                    workload.step(&mut engine);
                }
                workload.assert_mirror_matches_engine(&engine);
                black_box(&engine);
            },
            BatchSize::LargeInput,
        )
    });
    group.finish();
}

/// Per-op-kind chunks. Each isolates one arm of the mix with a fresh
/// steady state, so a number named "move" is only moves.
fn bench_per_op(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("engine/per_op");
    group.throughput(Throughput::Elements(OPS_PER_GROUP));

    let arms: [(&str, ArmDriver); 4] = [
        ("gtc_place", Workload::step_gtc),
        ("ioc_take", Workload::step_ioc),
        ("cancel", Workload::step_cancel),
        ("move", Workload::step_move),
    ];
    // Refill rule per arm: place/move don't drain the book; cancel
    // consumes ~1 order/op (refills hover just under the threshold);
    // IOC fills kill ~0.05 makers/op but sweep liquidity, so it also
    // refills. The gtc_place arm has NO refill: it is a pure insertion
    // workload into a GROWING book (1k → ~51k live orders) — that meaning
    // is disclosed here, not tunable away; the mixed group measures
    // steady-state placement.
    let refill: [usize; 4] = [0, PRELOAD_ORDERS, PRELOAD_ORDERS, 0];
    for ((name, driver), refill_below) in arms.iter().zip(refill) {
        group.bench_function(*name, |b| {
            b.iter_batched(
                || {
                    let mut workload = Workload::new(21);
                    let mut engine = Workload::fresh_engine();
                    workload.bootstrap(&mut engine);
                    (workload, engine)
                },
                |setup| {
                    let refills = run_arm(setup, OPS_PER_GROUP, *driver, refill_below);
                    black_box(refills);
                },
                BatchSize::LargeInput,
            )
        });
    }

    group.finish();
}

criterion_group!(benches, bench_reference_mix, bench_per_op);
criterion_main!(benches);
