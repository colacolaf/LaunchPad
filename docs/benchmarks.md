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
