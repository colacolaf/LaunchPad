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

Market data feeds (full order log, L2 market data, BBO, trades), clearing and settlement, reporting, clustering, FIX/REST gateways, NUMA-aware config. We don't need these, but they reveal what a "complete" exchange needs — useful for scoping our VENUE layer.

## Source

- README (fetched from `raw.githubusercontent.com/exchange-core/exchange-core/master/README.md`, Aug 21, 2026)
- GitHub API repo metadata (Aug 21, 2026)
