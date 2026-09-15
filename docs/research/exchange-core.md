# Research: exchange-core

> **Repo:** github.com/exchange-core/exchange-core · **Language:** Java · **Stars:** ~2,600 · **License:** Apache 2.0 · **Verified:** Aug 21, 2026 (GitHub API)

## What it is

An open-source **market exchange core**: orders matching engine, risk control and accounting module, disk journaling and snapshots module, and trading/admin/reports API. Built on LMAX Disruptor, Eclipse Collections, Real Logic Agrona, OpenHFT Chronicle-Wire, LZ4 Java, and Adaptive Radix Trees.

This is **the benchmark reference** for Launchpad's CORE layer — the thing we calibrate our numbers against.

## Why it's the bar

- **~150ns per match** for large market orders.
- **5M operations/sec** on a single order book, on **10-year-old hardware** (Intel Xeon X5690, 2010-era).
- **< 1ms worst wire-to-wire** latency at 1M+ ops/sec.
- Designed for: 3M users / 10M accounts, 100K order books / 4M pending orders.

## Published latency table (single order book, µs)

| rate | 50% | 90% | 95% | 99% | 99.9% | 99.99% | worst |
|---|---|---|---|---|---|---|---|
| 125K | 0.6 | 0.9 | 1.0 | 1.4 | 4 | 24 | 41 |
| 250K | 0.6 | 0.9 | 1.0 | 1.4 | 9 | 27 | 41 |
| 500K | 0.6 | 0.9 | 1.0 | 1.6 | 14 | 29 | 42 |
| 1M | 0.5 | 0.9 | 1.2 | 4 | 22 | 31 | 45 |
| 2M | 0.5 | 1.2 | 3.9 | 10 | 30 | 39 | 60 |
| 3M | 0.7 | 3.6 | 6.2 | 15 | 36 | 45 | 60 |
| 4M | 1.0 | 6.0 | 9 | 25 | 45 | 55 | 70 |
| 5M | 1.5 | 9.5 | 16 | 42 | 150 | 170 | 190 |
| 6M | 5 | 30 | 45 | 300 | 500 | 520 | 540 |

**Read this table for the shape, not just the numbers:** p50 stays sub-microsecond while the tail (p99.99) degrades as load rises. That's the honest behavior of a real matching engine. Our benchmarks should reproduce this shape or explain why not.

## The benchmark methodology (we copy this discipline, not the code)

- Single symbol order book.
- 3,000,000 inbound messages: **9% GTC orders, 3% IOC orders, 6% cancel commands, 82% move commands**. ~6% of messages trigger one or more trades.
- 1,000 active user accounts; ~1,000 active limit orders in ~750 price slots.
- Latency figures only for **risk processing + matching** (not network, IPC, journaling).
- Test data is not bursty (constant interval between commands, 0.2–8µs depending on target throughput).
- BBO prices don't change significantly; no avalanche orders.
- **No coordinated omission** — any processing delay affects measurements for following messages.
- GC triggered prior/after every benchmark cycle (3M messages).
- Hardware: RHEL 7.5, network-latency tuned-adm profile, dual X5690 (6 cores, 3.47GHz), one socket isolated + tickless, spectre/meltdown protection disabled. Java 8u192.

## Features that matter for us

- **Event sourcing:** disk journaling + journal replay, state snapshots (serialization) + restore, LZ4 compression.
- **Lock-free and contention-free** matching and risk control (LMAX Disruptor pipeline; each CPU core owns a processing stage/account shard/symbol shard).
- **No floating-point arithmetic** — no loss of significance possible. (We adopt this: integer scaled values.)
- Matching + risk control are **atomic and deterministic**.
- Two risk modes: direct-exchange and margin-trade.
- Maker/taker fees in quote-currency units.
- Two order-book implementations: "Naive" (simple) and "Direct" (performance).
- Order types: IOC, GTC, FOK-B (fill-or-kill budget).
- Order operations: place, move (price change without replace), cancel. Move is the priority operation (~0.5µs); cancel ~0.7µs; place ~1.0µs.
- Low GC pressure, object pooling, single ring buffer.

## What to steal vs. what to skip

**Steal:**
- The benchmark methodology (data mix, percentile reporting, hardware disclosure) — verbatim in spirit.
- Event-sourcing + snapshot/replay design.
- No-floating-point accounting.
- The invariant set and the "atomic + deterministic" framing.
- Order-operation model (place / move / cancel) — move is a real-world HFT concern most toy engines miss.

**Skip:**
- The Java/LMAX stack itself (if we go Rust — see `docs/stack.md`).
- The scale targets (3M users / 100K books) — those are production exchange targets, not ours.

## Not-yet-built (their TODOs — our opportunities)

Market data feeds (full order log, L2 market data, BBO, trades), clearing and settlement, reporting, clustering, FIX/REST gateways, NUMA-aware config. We don't need these, but they reveal what a "complete" exchange needs — useful for scoping our VENUE layer. Also on their TODO: **IOC_BUDGET and plain FOK support** — plain quantity-based FOK is something *we* have and *they* don't.

## Re-read against the finished engine surface (2026-09-14 — Phase 2 opening read)

Re-verified against primary sources (README + `IOrderBook.java` + `OrderBookNaiveImpl.java`, master branch, fetched 2026-09-14). The methodology section above and `docs/benchmarks.md`'s capture both check out against the source unchanged. Four surface findings, none of them in the August note:

1. **Their move is marketable; ours rejects (`WouldCross`).** `moveOrder` removes the order, re-prices it, runs `tryMatchInstantly` against the opposite side at the new price — a moved order is a *taker*: it sweeps what it crosses, fills, and the remainder rests at the new price (partial fills preserved via `order.filled`). Same-price moves re-queue at the tail (remove-then-put); the interface doc notes "if 0 or same - order will not moved". Our Phase 0 decision rejects repricing into the opposite side outright — the no-cross invariant is preserved structurally rather than by matching. A marketable move is composable in our model as cancel+place, at the cost of time priority.
2. **Their move risk gate is `price > reserveBidPrice` → `MATCHING_MOVE_FAILED_PRICE_OVER_RISK_LIMIT`.** The reserve is captured at place; moves therefore *never touch balances* — that is the entire point of `reservePrice` (bid moves are bounded by the worst-case reserve, so no risk re-check is needed, which is why their move is the ~0.5µs priority op). Our engine instead checks `free + this order's own recycled lock` at move time — strictly more flexible (no reserve cap fixed at place) but the move path performs a funds check theirs doesn't. A deliberate, documented Phase 1 deviation (flag (a) closure); it **will** show in benchmark comparisons.
3. **They have a fourth operation: `reduceOrder`** — decrease the order's size by N lots (clamped to remaining; removal when fully reduced), emitting a reduce event. A partial cancel. Our surface lacks it. Money-relevant (a partial lock release), so it belongs with the Phase 3 journal/risk work, not a Phase 2 feature drop.
4. **Cancel/move/reduce are uid-gated** (`order.uid != cmd.uid → MATCHING_UNKNOWN_ORDER_ID`). Our cancel/move take only the order id — fine while input is trusted (tests), load-bearing the moment the venue (Phase 4) lets strangers send commands.

Also confirmed: their duplicate-id gate runs *after* matching ("can match, but can not place" — only the resting remainder is rejected); ours rejects the whole place with full saga compensation. Ours is stricter and simpler; theirs exploits matching-before-indexing.

### Adopt/skip list (the deliverable)

| Item | Verdict | When / why |
|---|---|---|
| Benchmark methodology (mix, no-burst, no coordinated omission, percentile tables, hardware disclosure) | **ADOPT** | Phase 2 — already captured in `docs/benchmarks.md`; re-verified against source. The harness copies it: single symbol, 9/3/6/82 mix, ~1,000 live orders in ~750 slots, 1,000 accounts. |
| Move-heavy workload weighting (move as the priority op) | **ADOPT** | Phase 2 — the 82% move share is the workload that made move their fastest op; it stresses our re-queue + funds-check path hardest. |
| HDR-style percentile reporting (p50…p99.99 + worst, not averages) | **ADOPT** | Phase 2 — criterion configured to report the full percentile table. |
| `reduceOrder` (partial cancel) | **ADOPT — deferred** | Phase 3 — a money-relevant lifecycle op (partial lock release); lands with journaling where the reduce event belongs. |
| uid ownership check on cancel/move/reduce | **ADOPT — deferred** | Phase 4 — becomes load-bearing when untrusted input arrives at the API; logged now so it isn't lost. |
| Marketable moves (move sweeps the opposite side) | **SKIP** | Core-semantics change = new feature, and Phase 2 is performance-only by guardrail. Composable as cancel+place today; revisit only if the venue needs it. |
| `reservePrice` move-without-risk-check model | **SKIP (as replacement)** | Our move-time funds check is more flexible; their design trades that for move latency. Revisit only if Phase 2 profiling points at the funds check. |
| FOK-B (budget) semantics | **SKIP** | Our quantity-based FOK matches the phases.md mandate ("fill entirely or not at all"); FOK-B is a possible venue-era feature. (They haven't built plain FOK at all — their TODO.) |
| Duplicate-id partial-match nuance | **SKIP** | Our whole-place rejection with guaranteed compensation is stricter and simpler to reason about. |

**Comparability disclosure (goes in the methodology when numbers are published):** our per-op cost is not apples-to-apples with theirs — our moves carry a risk check, our cancels release locks, and our fills settle from locks; theirs defers all risk handling to a place-time reserve. The published Phase 2 numbers must state this next to the reference table.

## Source

- README (fetched from `raw.githubusercontent.com/exchange-core/exchange-core/master/README.md`, Aug 21, 2026)
- GitHub API repo metadata (Aug 21, 2026)
