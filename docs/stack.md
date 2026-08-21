# Tech stack — decision matrix

**Status:** DECIDED — **Rust core + Python sim** (resolved 2026-08-21, see `docs/record/decision-log.md` and the deep-research report `docs/research/rust-vs-java.md`). This doc keeps the full comparison for the record and for re-evaluation if priorities change.

The original recommendation was **Rust core + Python sim**; the research confirmed it. The call was made on the **learning-depth axis** (the #1 project priority): building the engine from scratch in Rust — with no mature Rust reference to copy — is the deeper learning, and Rust's compile-time data-race safety + no-GC hot path + strongest interview story reinforce it. Java remains the stronger choice *only if* direct side-by-side benchmark parity with exchange-core ever becomes the top priority.

## The decision

| Layer | Recommended | Why | Alternative |
|---|---|---|---|
| CORE (exchange) | **Rust** | Memory safety + zero-cost abstraction; the modern HFT language; strongest AI-assist learning path; hardest-but-best for the story | Java (matches exchange-core exactly), C++ (industry standard, highest difficulty) |
| SIM (science) | **Python** | Research velocity, ecosystem (notebooks, numpy/pandas), ABIDES itself is Python | — |
| VENUE | Python/TS | Web app + leaderboard | — |

## Rust vs Java for the core

| Dimension | Rust | Java |
|---|---|---|
| **Learning curve** | Steeper (ownership/borrowing is the gate) | Gentler; more familiar OO |
| **Reference match** | No direct Rust reference; must translate exchange-core's *ideas* | exchange-core is Java — closest possible comparison; can run its benchmark methodology against it directly |
| **Performance ceiling** | Zero-cost abstractions; no GC pauses; fine control over memory layout | LMAX Disruptor + no-GC techniques get to 5M ops/sec, but requires careful GC discipline |
| **Concurrency model** | Ownership makes data-race-free concurrency *provable at compile time* | Locks/atomics + Disruptor; races caught at runtime |
| **Event sourcing / journaling** | No built-in; use serde + a journal crate | exchange-core shows a full pattern (LZ4 journal + snapshots) to copy |
| **AI-assist quality** | Excellent — Rust is well-represented in training data; compiler is a strict teacher | Excellent |
| **Interview story** | "I learned systems programming in Rust" — strong signal; harder = better story | "I built it in Java" — fine, but less distinctive |
| **Risk** | Borrow checker fights you early; slower to first working engine | GC tuning + Disruptor are their own deep rabbit holes |

### Recommendation

**Rust**, unless the goal of a *direct, apples-to-apples benchmark against exchange-core* outweighs everything else. If the benchmark comparison is the #1 priority, Java is defensible — you could literally run the same workload on both and publish side-by-side numbers. That's a powerful artifact either way.

**Decision rule:** at Phase 0, write a minimal order book in *both* — 1 weekend each — and pick based on which one you can actually reason about under the "explain every line" rule. The language you can fully explain is the language that survives the interview.

## The Python sim rationale

- ABIDES (the academic reference) is Python — matching its ecosystem lowers the translation cost.
- Experiment harness + notebooks + pandas = fast iteration on the science layer, where correctness of *research* matters more than raw speed.
- The hot path (matching) is in Rust; the sim talks to it over the ITCH/OUCH-style feed, so Python's speed is not the bottleneck.

## Non-negotiables regardless of choice

- **No floating point in the accounting/matching path** (integer scaled values, like exchange-core).
- **Deterministic replay** must be provable.
- **Benchmark methodology published with every number.**
- The "explain every line" rule applies to whatever language is chosen.

## Confirmation checklist

- [ ] Minimal order book written in Rust (Phase 0)
- [ ] (If Java considered) minimal order book written in Java for comparison
- [ ] Final call logged in `docs/record/decision-log.md` with rationale
