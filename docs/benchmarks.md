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
