# Architecture

The four layers of Launchpad, how they connect, and the design decisions that matter.

```
LAUNCHPAD
├─ CORE (Rust) — the exchange
│   ├─ Order book (price-time priority; limit / GTC / IOC / FOK / market)
│   ├─ Matching engine (continuous double auction)
│   ├─ Risk & accounting (balances, position limits, maker/taker fees, margin modes)
│   ├─ Event sourcing (disk journal, snapshots, deterministic replay)
│   └─ API layer (JSON/WebSocket for the venue; ITCH/OUCH-style feed for the simulator)
├─ SIM (Python) — the science
│   ├─ Agent-based market simulator (ABIDES-style)
│   ├─ Experiment harness
│   └─ Research notebooks → the paper
├─ VENUE (Python/TS) — the launch
│   ├─ Paper-trading accounts, leaderboard, competition engine
│   └─ The hosted competition (spring 2027)
└─ RECORD (repo + docs) — the proof
    ├─ Public README with honest benchmark results + CI badge
    ├─ Decision log (weekly)
    └─ Quarterly write-ups incl. the bad quarters
```

## CORE (Rust) — the exchange

The hot path. Everything here is about correctness first, then latency.

### Order book
- **Price-time priority:** best price first; at the same price, earliest arrival first. This is the invariant everything else hangs off.
- Order types: **limit, GTC** (good-till-cancel), **IOC** (immediate-or-cancel), **FOK** (fill-or-kill), **market**.
- Data structure choice is a Phase 2 concern (price-level buckets vs. per-order lists; adaptive structures). Start simple and correct (Phase 1), benchmark, then optimize (Phase 2).

### Matching engine
- Continuous double auction: an incoming order matches against the opposite side of the book at the best price until filled or the book is exhausted; unmatched remainder rests (for GTC/limit) or cancels (IOC/FOK).
- Correctness invariants (must have property tests):
  - Price-time priority is preserved.
  - The book never crosses (best bid < best ask after processing).
  - Sum of fills = traded quantity (no leakage).
  - Deterministic: same input sequence → same output state.

### Risk & accounting
- Balances per user per currency/asset.
- Position limits (per symbol, per user).
- Maker/taker fees (in quote-currency units, like exchange-core).
- Margin modes (direct-exchange vs. margin-trade) — Phase 3+.
- **No floating point** in the accounting path (exchange-core uses integer scaled values — this is a hard, correct-by-construction choice).

### Event sourcing
- Every order/command is an event appended to a **disk journal**.
- Periodic **snapshots** of full state; restore = latest snapshot + replay of events after it.
- **Deterministic replay:** replay produces identical state — this is proven by a test, not claimed.
- Compression (exchange-core uses LZ4) is a later optimization.

### API layer
- JSON/WebSocket for the venue (humans + the web app).
- ITCH/OUCH-style feed for the simulator (machine, low-level, modeled on NASDAQ's published protocols — this is what ABIDES speaks).

## SIM (Python) — the science

- **Agent-based market simulator** modeled on ABIDES: thousands of agents (random, trend-following, market-making), an exchange agent, configurable **pairwise network latencies** between every agent and the exchange.
- Message-based, ITCH/OUCH-style.
- **Experiment harness:** run a scenario N times with a fixed seed; record reproducible results.
- Research notebooks → the paper (see `docs/paper.md`).

## VENUE (Python/TS) — the launch

- Paper-trading accounts (no real money).
- Leaderboard + competition engine (rounds, scoring, results publication).
- The hosted competition in spring 2027 (see `docs/venue.md`).

## RECORD — the proof

- Public README with honest benchmark results + methodology + live CI badge.
- Decision log (weekly) + quarterly write-ups, bad quarters included (see `docs/record/`).

## Key cross-cutting decisions

1. **Determinism is the backbone.** Event sourcing + deterministic replay means every result is reproducible and auditable. Nothing in the core is allowed to break this (no wall-clock dependence in matching, no randomness in the hot path).
2. **Honest benchmarks.** Methodology is published with every number (see `docs/benchmarks.md`).
3. **Correct before fast.** Phase 1 is correctness; Phase 2 is speed. Never optimize what isn't proven correct.
4. **Educational framing.** The venue is paper trading only; the core is a learning artifact, not a production money system (see `docs/guardrails.md`).
