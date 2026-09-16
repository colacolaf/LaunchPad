# Benchmarks — targets and methodology

> **The rule that makes this project honest:** publish the benchmark methodology (hardware, data mix, measurement method) with the numbers. A number without methodology is a claim; with methodology it's evidence. This is the anti-vibe-coding rule that makes the project audit-proof.

## Targets

| Metric | v1 (correct) | v2 (optimized) | Stretch | Reference |
|---|---|---|---|---|
| Throughput | 50k ops/sec | 500k ops/sec | 1M+ ops/sec | exchange-core: 5M single order book (10-yr-old hardware) |
| Match latency (p99) | < 1ms | < 100µs | < 10µs | exchange-core: ~150ns/match |
| Replay determinism | — | identical state | — | required |
| Simulator agents | 1k | 10k | 50k+ | ABIDES: tens of thousands |

## What "honest" means (the methodology template)

Every published benchmark must include:

1. **Hardware:** CPU model, cores, clock, RAM, OS + kernel, CPU governor / isolation settings, whether other processes ran.
2. **Data mix:** exactly what the inbound message stream contained. exchange-core's published mix is the reference: 3,000,000 messages = 9% GTC orders, 3% IOC orders, 6% cancel commands, 82% move commands; ~6% of messages trigger one or more trades; ~1,000 active limit orders in ~750 price slots; 1,000 active accounts.
3. **Measurement method:** what is measured (risk processing + matching only? wire-to-wire? including journaling?), how percentiles are computed (HDR histogram), whether the GC was controlled, whether there's coordinated omission.
4. **The raw numbers:** percentile table (p50 / p90 / p95 / p99 / p99.9 / p99.99 / worst), not just an average.
5. **Date + environment fingerprint:** when it ran, on what.

A benchmark that omits any of these is not a published result — it's a draft.

## Reference methodology (exchange-core, from its README — the bar we calibrate to)

- Single symbol order book.
- 3,000,000 inbound messages: 9% GTC, 3% IOC, 6% cancel, 82% move. ~6% trigger trades.
- 1,000 active user accounts; ~1,000 active limit orders in ~750 price slots.
- Latency figures cover risk processing + matching only (not network, IPC, or journaling).
- Test data is not bursty (constant interval between commands).
- BBO prices don't change significantly; no avalanche orders.
- No coordinated omission; GC triggered before/after each 3M-message cycle.
- Hardware: RHEL 7.5, dual Intel Xeon X5690 (6 cores, 3.47GHz), one socket isolated + tickless, spectre/meltdown protection disabled, Java 8u192.

Their published single-order-book latency table (µs):

| rate | 50% | 90% | 95% | 99% | 99.9% | 99.99% | worst |
|---|---|---|---|---|---|---|---|
| 125K | 0.6 | 0.9 | 1.0 | 1.4 | 4 | 24 | 41 |
| 500K | 0.6 | 0.9 | 1.0 | 1.6 | 14 | 29 | 42 |
| 1M | 0.5 | 0.9 | 1.2 | 4 | 22 | 31 | 45 |
| 5M | 1.5 | 9.5 | 16 | 42 | 150 | 170 | 190 |

Note the shape: **p50 stays sub-microsecond while p99.99 degrades** as load rises. That's the honest tail behavior real matching engines exhibit — our benchmarks should show the same shape or explain why not.

## Launchpad methodology (v1, written 2026-09-15 — before baseline v1 was recorded)

**Scope measured.** The full client-to-ack path through the public `Engine` API: command construction by the workload generator, risk/lock handling, matching, settlement, and the engine's response — **not** the bare matching call. This is deliberately *broader* than the reference's "risk + matching only" scope; the comparability disclosure below is the standing rule from the adopt/skip list.

**Workload (seeded, deterministic).** splitmix64-seeded command stream, seed fixed in source (`0xFEED` for latency, `11`/`21` for criterion groups). Mix: 9% GTC place / 3% IOC / 6% cancel / 82% move — the reference's ratios exactly. Steady state: ~1,000 live GTC orders (preloaded through the same `place` path), 1,000 funded accounts, one symbol, ±375-tick price band around a 100.0 mid (~750 levels). GTC prices jitter outside the spread so the trade rate stays IOC-driven (~5% of ops fill — disclosed: the reference reports ~6% under its own pricing; we do not tune ours to match). No bursts: commands execute back-to-back, generator bookkeeping included in every sample.

**Liveness mirror.** The generator tracks what *should* be live and repairs only on reported failures; a run aborts (rather than publishes) if the mirror ever diverges from the engine's live map — verified exact at end of every run, plus Σ(free+locked) = deposits conservation per currency.

**Measurement.** Latency: a `harness = false` binary times **every** op (`Instant::now()` per op, no sampling, no harness); exact percentiles over the full sorted sample set. Throughput: criterion 0.8.2 (`default-features = false`), 50k-op batches re-bootstrapped per iteration so batches are comparable; per-op-kind chunks hold the book at steady state by placing refills (draining arms without refill collapse to empty-book degenerates — measured 4.7 ns/cancel once; the number was discarded, the rule was written). Percentiles are computed from sorted vectors, not HDR histograms — matches the reference's own approach.

**Determinism proof per run.** Every latency run executes the seeded stream **twice** and fails loudly if the full-state digests (sorted live-order table + all 2,000 balance cells) diverge. Same seed → same commands → same final state is asserted, not assumed.

**Environment & profile.** Release profile `lto = "fat"`, `codegen-units = 1`; toolchain pinned in `rust-toolchain.toml` (1.96.0; the local Homebrew rust ignores the pin — disclosed). No CPU pinning, default power management — a laptop, measured like a laptop.

### Baseline v1 — 2026-09-15 (first published number)

Hardware: Apple M1 (8 cores), 16 GB, macOS (Homebrew rustc 1.96.0). 50,000 mixed ops, seed `0xFEED`, two runs per invocation. Full client-to-ack scope (see above).

**Reference mix, latency (ns/op):**

| run | mean | p50 | p90 | p99 | p99.9 | max |
|---|---|---|---|---|---|---|
| 1 | 292 | 291 | 375 | 709 | 1,459 | 33,250 |
| 2 | 290 | 291 | 375 | 750 | 1,500 | 43,666 |

Mix realized (run 1): gtc 4,490 / ioc 1,544 / cancel 2,973 / move 40,428 (+565 rejected — clamped no-ops or targets that died by fill; the counter does not decompose them) — 9.0/3.1/6.0/82.0%. Trades: 2,564 (5.1% of ops), 68,103 lots. Cancel-miss rate: **0** across 100,000 ops. Determinism: digests matched in both invocations.

**Throughput (criterion, 50k-op batches):**

| group | per-op | ops/sec |
|---|---|---|
| mixed 9/3/6/82 | ~310 ns | ~3.2 M |
| gtc_place (growing book, 1k→51k) | ~316 ns | ~3.16 M |
| ioc_take (steady-state, ~2.5% refills) | ~662 ns | ~1.51 M |
| cancel (steady-state, ~50% refills) | ~425 ns | ~2.35 M |
| move | ~265 ns | ~3.78 M |

Cross-checks (all pass): criterion mixed ≈ 1/mean of the latency table (3.2 M vs 3.4 M ops/s); IOC per-op ≈ the latency table's p99 (IOC dominates the tail); move ≈ the table's p50 (moves dominate the mix).

**Reading the numbers honestly.** Comparability disclosure (the standing rule from the adopt/skip list): our per-op cost includes risk handling on every op and generator bookkeeping in every sample — costs exchange-core defers to a place-time reserve and excludes entirely — so these numbers read *conservative* next to theirs, never flattering. p50 ≈ 0.3 µs at ~1,000 live orders on an M1 laptop — sub-microsecond, in the reference's ballpark at comparable load (their p50 0.5–0.6 µs on 2012 Xeons under rate-limited load). The tails differ in kind: our p99.9 is 1.5 µs, ~4× p50 — flatter than the reference's rate-driven 30×+ tail blowup, because we measure throughput-shaped batches, not a rate-limited stream with coordinated-omission-free pacing. That's a scope difference to fix in a later methodology revision, not a claim of superiority. This is a **baseline**, not a target — optimization starts only after profiling, per the phase plan.

**Reproducibility:** `cargo bench --bench latency` (full) / `LAUNCHPAD_BENCH_OPS=2000 cargo bench --bench latency` (smoke) / `cargo bench --bench throughput`. No flags, no tuning; everything above is in the repo.

### Baseline v1.1 — 2026-09-16 (wider sample base; adds p99.99 + cross-architecture determinism)

Hardware: same as v1 (Apple M1, 16 GB, Homebrew rustc 1.96.0). **1,000,000 mixed ops**, seed `0xFEED`, two runs per invocation. This table supersedes v1's for optimization comparisons; v1 is retained as published.

**Reference mix, latency (ns/op):**

| run | mean | p50 | p90 | p99 | p99.9 | p99.99 | max |
|---|---|---|---|---|---|---|---|
| 1 | 280 | 250 | 334 | 667 | 1,417 | 5,459 | 109,375 |
| 2 | 277 | 250 | 334 | 667 | 1,416 | 6,583 | 135,875 |

Mix realized (run 1, gtc excludes the 1,000 bootstrap places): gtc 89,776 / ioc 30,014 / cancel 60,059 / move 807,512 (+12,639 rejected — clamped no-ops or targets that died by fill) — 9.0/3.0/6.0/82.0%. Trades: 49,284 (4.9% of ops), 1,308,304 lots. Cancel-miss rate: **0** across 2,000,000 ops. Determinism: digests matched in both runs (`0xc3bea4a3…`).

**What changed and why it's honest:**

- **p99.99 is sample-gated, not aspirational.** The harness now prints p99.99 only when N ≥ 100,000 — ten samples must sit above the threshold before the number means anything. At v1's 50k it rested on ~5 samples, so the line was omitted rather than fudged; `MIN_SAMPLES_P9999` in `core/benches/latency.rs` is the enforced rule.
- **The tail is real and short.** p99.99 ≈ 5.5–6.6 µs (~22× p50), p99.9 ≈ 5.7× p50, max ≈ 437× p50 — at 1M ops the extreme max is scheduler noise, not engine work, which is exactly why the policy is percentile tables and never a max-only claim. Still throughput-shaped batches (v1's disclosed scope gap): a rate-limited, coordinated-omission-free revision remains future work.
- **Sample base shifts the body.** p50/p90/p99 moved down from v1 (291/375/709–750 → 250/334/667) as warm-up amortized over 20× more samples; runs 1 and 2 agree within 1 ns through p99.9. Both tables are kept: v1's is what 50k ops resolves, v1.1's is the better estimate.
- **Cross-architecture determinism.** The CI smoke (ubuntu-24.04 x86_64 GitHub runner) and this M1 both ran the 2k-op stream: identical full-state digest `0xea9d71ec0f18e3a3` — same seed, same final state on two ISAs. The same CI job reconfirmed the no-numbers-from-CI rule the hard way: two runs on the shared VM swung p99.9 from 2,845 to 5,010 ns.

**Reproducibility:** `LAUNCHPAD_BENCH_OPS=1000000 cargo bench --bench latency` (~1 min locally). p99.99 appears only above the sample floor; smoke runs omit it by design.

### Profiling methodology (Phase 2 optimization — written 2026-09-16, before the first profile was collected)

**Purpose: attribution, not numbers.** Profiles answer one question — *where do the baseline's nanoseconds go?* — so that optimization is one falsifiable hypothesis per session, never speculative tweaking. Latency/throughput **numbers** for any before/after claim come only from the canonical harness above (same seed, same profile); a profiler's sampled shares are search guidance, never published as performance results.

**Binary.** The latency bench built with the canonical release profile (`lto = "fat"`, `codegen-units = 1`) **plus debug symbols** via `RUSTFLAGS="-g"` into a **separate target dir** (`CARGO_TARGET_DIR=target-prof`) — the canonical `target/` artifacts stay untouched, so no symbolication support leaks into recorded builds. The `-g` build may cost a few percent vs the profile it profiles; that is disclosed and irrelevant — shares, not absolutes, are read.

**Tool.** macOS `sample` (1 ms interval, ~10 s window covering both seeded runs of one invocation at `LAUNCHPAD_BENCH_OPS=20,000,000`). Sampling, not cycle-accurate; hot frames are read as *percent of on-CPU samples*, cross-checked between two independent invocations for stability. LTO + inlining can blur attribution at leaf boundaries — shares steer the search, source reading confirms.

**Session rule.** Profile → ground the top engine frames in source → **one** hypothesis with a **written prediction** (which number moves, by roughly how much, which must not move) → implement → all 100 tests green → same-harness before/after → decision row. A hypothesis that fails its prediction is recorded as a result (negative results are included), not retried until it flatters.

**Session #1 (2026-09-16) — H1: de-box `best()`. NEGATIVE RESULT.** Two stable invocations attributed ~4.4% of on-CPU samples to `BookSide::best` (boxed-iterator head); H1 predicted p50 250→~235–248 ns with digest/counters/tests unmoved. After: p50 250→250 ns (all 100 tests green, digest `0xc3bea4a3…` byte-identical, counters identical) — the share was attribution blur: 1 ms sampling + LTO frame blur, and allocator reuse makes a hot same-size-class box nearly free. The change ships as a strict simplification of the hot gate with **no performance claim**; the prediction is what failed, and the prediction is the record. (Run-mean swings of ±15% within this same session, machine hot from profiling, are why the written-prediction rule exists.)

## Testing & correctness discipline (non-negotiable)

- Every component ships with tests. **Unit + property-based** tests for order-book invariants:
  - Price-time priority is preserved.
  - No crossed book.
  - Sum of fills = traded quantity.
- **Determinism is proven, not claimed:** replay the journal, diff state. Same input → same output. There is a dedicated replay-determinism test.
- **No fake green:** CI runs tests + benchmarks; the README badge is live, not a screenshot.
- **Negative results are published** (in the paper and the record). Honest failure is the rare signal; that's the point.

## Benchmark harness requirements (Phase 2)

- Reproducible: fixed seed, fixed data generator, scripted run.
- Reports percentiles, not averages.
- Configurable workload (mix, rate, symbol count) matching the reference where comparable.
- Lives in the repo so anyone can re-run it (`bench/` or `criterion`-based for Rust).
- CI runs a smoke benchmark (not the full suite) so the badge stays honest without slowing every commit.
