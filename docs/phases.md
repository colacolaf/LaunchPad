# Phases 0–7

Budget: ~8 hrs/week, weekends only (Sat deep-focus sessions + Sun afternoon; other commitments keep their own slots — this is the flexible layer).

> **Working rules:** Every session has a one-line goal from this table. Never open-ended sessions. Scope changes get a 24-hour cool-off before adoption. A phase is done when its done-criteria are met — not when the calendar says so.

## Phase table

| Phase | When (2026–27) | Goal | Deliverable / Done-criteria | Stop condition |
|---|---|---|---|---|
| **0. Foundations** | Sep | Rust crash course (AI agent as tutor) + order-book mechanics; study exchange-core + ABIDES + LEAN | Minimal order book in Rust with unit tests | When you can explain price-time priority in 3 sentences and your tests pass |
| **1. Matching engine v1** | Oct | Correct, functional matching | Order types (limit/GTC/IOC/FOK/market), unit + property tests passing | All correctness invariants green |
| **2. Benchmark & optimize** | Nov–Dec | Measured performance | Benchmark harness: v1: 50k ops/sec → v2: 500k → stretch: 1M+ ops/sec; latency percentiles (p50/p99) | Diminishing returns > 2 sessions at a plateau; note the number honestly |
| **3. Risk + event sourcing** | Jan | Production-grade depth | Balances, position limits, fees; journal + snapshot + replay; replay determinism test | Replay produces identical state — proven, not claimed |
| **4. API + venue v1** | Feb | A place people can trade | JSON/WebSocket API, CLI demo, paper accounts | A stranger can place an order without your help |
| **5. Simulator** | Mar | The science engine | ABIDES-style agent simulator (thousands of agents, configurable latency), 2+ experiments run | Experiments produce reproducible results |
| **6. Host the competition** | Apr–May | The launch | Recruit participants; leaderboard; run it; write-up | ≥10 traders, ≥1 full round, results published |
| **7. Paper + record + internships** | Summer 2027 | The payoff | Research paper draft (B) with negative results; 12-month record write-up (C); internship push | Paper has reproducible code; record has no missing weeks |

## Per-phase detail

### Phase 0 — Foundations (Sep)
- **Learn:** Rust crash course (AI agent as tutor). Order-book mechanics: price-time priority, bid-ask spread, limit vs market orders, GTC/IOC/FOK semantics.
- **Study:** exchange-core (matching + event sourcing patterns), ABIDES (agent architecture), LEAN (platform structure). Notes land in `docs/research/`.
- **Decide:** ~~Rust vs Java → `docs/stack.md`, log the call in the decision log.~~ — **Decided 2026-08-21: Rust committed** (write-both-then-pick rule dropped). See `docs/stack.md`.
- **Deliverable:** minimal order book in Rust with unit tests.
- **Done when:** you can explain price-time priority in 3 sentences and your tests pass.

### Phase 1 — Matching engine v1 (Oct)
- **Build:** order book + matching engine with limit/GTC/IOC/FOK/market order types.
- **Test:** unit + property tests for all invariants (price-time priority, no crossed book, sum of fills = traded quantity).
- **Done when:** all correctness invariants green.

### Phase 2 — Benchmark & optimize (Nov–Dec)
- **Build:** benchmark harness with published methodology (`docs/benchmarks.md`).
- **Optimize:** v1 50k → v2 500k → stretch 1M+ ops/sec; report p50/p99/p99.99.
- **Plateau rule:** 2 sessions at a plateau → publish the honest number and move on.
- **Done when:** harness runs reproducibly, methodology published, honest number recorded.

### Phase 3 — Risk + event sourcing (Jan)
- **Build:** balances, position limits, maker/taker fees; disk journal + snapshots + replay.
- **Test:** replay-determinism test (replay journal → identical state).
- **Done when:** replay produces identical state — proven by the test.

### Phase 4 — API + venue v1 (Feb)
- **Build:** JSON/WebSocket API, CLI demo, paper accounts.
- **Done when:** a stranger can place an order without your help.

### Phase 5 — Simulator (Mar)
- **Build:** ABIDES-style agent simulator (thousands of agents, configurable pairwise latencies), experiment harness.
- **Run:** 2+ experiments (see `docs/paper.md`).
- **Done when:** experiments produce reproducible results.

### Phase 6 — Host the competition (Apr–May)
- **Build:** leaderboard, competition engine, paper accounts.
- **Run:** recruit ≥10 traders, ≥1 full round, publish results.
- **Done when:** ≥10 traders, ≥1 full round, results published.

### Phase 7 — Paper + record + internships (Summer 2027)
- **Write:** research paper draft (B) with negative results; 12-month record write-up (C).
- **Push:** internship campaign (`docs/internships.md`).
- **Done when:** paper has reproducible code; record has no missing weeks.

## Threading with existing commitments

- **Team investment competition** (Sept 28 – Dec 4): Phases 1–2 run alongside. The market-microstructure knowledge strengthens the report. Keep it the priority in Oct–Nov; Launchpad is the flexible layer.
- **IMC Prosperity 2027** (~April): optional external validation — enter as a trader on their venue while hosting yours. Participation is open; prizes are university-restricted (verify before relying).
- **Coursework roadmap:** the vocabulary feeds both the team competition and Launchpad write-ups.

## Weekly rhythm (one window, one task)

- **Sat block 1:** core build (Rust)
- **Sat block 2:** sim/research or write-up
- **Sun afternoon:** tests, review, decision-log entry, next-week plan

## Definition of done (junior year)

- Matching engine + order book passing correctness tests
- Benchmark harness with published methodology and honest numbers (≥500k ops/sec target)
- Risk/accounting + event sourcing with proven replay determinism
- Paper-trading venue with API, used by ≥1 person who isn't you
- Competition hosted (≥10 traders, results published)
- 1–2 simulator experiments documented
- Decision log with zero missing weeks
- Paper draft (B) started, negative results included
