# Research: Rust vs Java for the Launchpad core

> **Status:** ✅ RESOLVED 2026-08-21 — **Rust committed** (write-both-then-pick tiebreaker dropped; see `docs/stack.md` + `docs/record/decision-log.md`). The research below is the pre-decision snapshot, retained for the record. The recommendation in the executive summary was adopted.
>
> **Stakes:** high — this locks the hot-path language for the whole project. **Output:** in-chat + this report.
> **Skills used:** `deep-research` (Deep mode) + `questions` (Full, to follow).
> **Verification note:** this environment's web search is filtered (Securly blocks `github.com` search); primary data was gathered from the exchange-core source/pom.xml via the GitHub API, the installed `rust-best-practices` skill chapters, and well-established language facts. Web-verified recent-developments claims are flagged as such.

## Executive summary

**Recommendation: Rust** — but not by a wide margin, and the decision hinges on which project property you weight most. Rust wins on the *interview story*, *AI-assist learning curve*, and *compile-time data-race safety*; Java wins on *direct apples-to-apples benchmarking against the exchange-core reference* and *shallowest learning curve to a first working engine*. ~~The plan's own rule — write a minimal order book in each (one weekend each), pick the one you can fully explain under the "explain every line" rule — is the right tiebreaker and should be honored before locking the call.~~ *(The write-both tiebreaker was superseded 2026-08-21: the user committed to Rust directly after this research confirmed the recommendation.)*

## Research angles

### 1. Steelman for Rust (the strongest case for Rust)

- **No GC pauses, deterministic memory.** The matching hot path has no stop-the-world collector. exchange-core achieves its numbers in Java *despite* the GC, via careful GC discipline (triggered before/after each benchmark cycle, object pooling, single ring buffer) — Rust gives you that property for free and removes a whole class of tuning the README itself admits is fragile ("newer Java 8 versions can have a performance bug").
- **Compile-time data-race safety (`Send`/`Sync`).** When Phase 2 introduces concurrency/lock-free structures, Rust's borrow checker makes data races *provable at compile time*. exchange-core's lock-free design in Java relies on careful human reasoning about memory visibility; Rust encodes that in the type system. The `rust-best-practices` skill's Chapter 9 documents the `Send`/`Sync`/`Arc`/`Mutex` model directly relevant to a pipelined matching engine.
- **Zero-cost abstractions + memory-layout control.** Iterators compile to tight loops (verified in Chapter 3 of the skill); you control stack vs heap explicitly. This is the *exact* toolkit HFT asks for, and it's the textbook argument for Rust in performance-critical systems.
- **The story.** "I learned systems programming and built a matching engine in Rust" is a meaningfully stronger interview signal than the same sentence ending in Java. The plan's whole thesis is that the project is unfakeable and interview-proof — Rust raises the ceiling on that property.
- **Strongest-in-class AI assist + a working `rust-best-practices` skill already in the repo.** Rust is heavily represented in training data; the borrow checker is a strict teacher that surfaces mistakes the AI would otherwise let through.

### 2. Steelman for Java (the strongest case against the default of Rust)

- **Direct, apples-to-apples benchmark parity with exchange-core.** This is the single strongest Java argument and the plan acknowledges it. exchange-core is Java 8 + Maven + LMAX Disruptor + Eclipse Collections + Agrona + OpenHFT + LZ4 (verified from its `pom.xml`). Choosing Java means you can run the *same benchmark methodology on the same workload* and publish side-by-side numbers — a uniquely powerful artifact. With Rust, your numbers are Rust-vs-a-Java-reference, which a skeptic can hand-wave ("different runtime, different GC, unfair").
- **A complete, battle-tested pattern to copy.** exchange-core ships a full event-sourcing + snapshot + LZ4 journal + lock-free pipeline design *in Java*. If you go Rust, you must *translate* those ideas (no direct Rust reference of comparable maturity). The risk of translation errors is real, and the "explain every line" rule bites harder when the line is your translation of someone else's clever lock-free Java.
- **Gentler learning curve to a first working engine.** Java's OO is more familiar; the borrow checker is a known early-stage friction. For a Phase 0 deadline (one month to a minimal order book), Java gets you to "working + tested" faster with less risk of a borrow-checker wall.
- **exchange-core's own benchmark discipline is the model regardless of language** — you copy the *methodology*, not the code. Java lets you copy the *implementation patterns* too.

### 3. Primary data (verified)

- **exchange-core is Java 8**, Maven, Apache-2.0, with JNA for thread affinity and a heavy specialized stack (LMAX Disruptor, Eclipse Collections, Real Logic Agrona, OpenHFT Chronicle-Wire, LZ4 Java, Adaptive Radix Trees). Source: `pom.xml` via GitHub API (Aug 2026).
- Its published numbers — 150ns/match, 5M ops/sec single book on a 10-yr-old Xeon X5690 — come with a documented methodology (data mix, percentile table, hardware, GC control). Source: README via `raw.githubusercontent.com` (Aug 2026).
- **Rust has no matching-engine reference of comparable maturity/benchmark disclosure** that is verifiable from here (web search filtered). This is a real gap, not a claim — flag it.
- The `rust-best-practices` skill's chapters 3 (performance: flamegraph, avoid redundant clones, stack/heap, zero-cost iterators) and 9 (pointers/thread-safety: `Send`/`Sync`, `Arc`/`Mutex`/`RwLock`) directly cover the toolset a matching engine needs. Source: installed skill (Apollo, MIT).

### 4. Recent developments / counter-evidence

- ⚠️ **Could not web-verify** recent (2025–26) Rust-in-HFT adoption claims or Rust matching-engine projects due to the Securly filter on web search. Treat any "Rust is taking over HFT" claim as unverified until re-checked on an unfiltered network. **Flagged gap.**
- The plan itself flags exchange-core's benchmark figures as "10-yr-old hardware" — meaning the *bar* is approachable on modern hardware in either language; the gap between Rust and Java at this scale is likely smaller than the gap between either and a naive implementation. That cuts *against* over-weighting raw language performance in the decision.

## Points of disagreement (surfaced explicitly)

| Disagreement | Both sides | Net read |
|---|---|---|
| Performance: Rust "is faster" | Rust has no GC + layout control; Java+Disruptor hits 5M ops/sec anyway | At our target scale (500k–1M ops/sec), both clear it comfortably; performance is **not** the deciding factor |
| Learning curve | Rust's borrow checker is a known early wall; Java is gentler | Java is faster to *first working engine*; Rust is a better *teacher*. Depends whether Phase 0's risk is "won't finish" or "won't learn deeply" |
| Reference parity | Java = direct side-by-side benchmark; Rust = translate ideas, no peer reference | Java wins this clearly; it's the single strongest Java argument |
| Story | "Built it in Rust" lands harder; "Java" is fine but common | Rust wins the narrative axis, which the plan weights heavily |

## Confidence assessment

- **High confidence:** Rust gives compile-time data-race safety + no GC + layout control (language facts, skill-verified). Java gives direct reference parity + a copyable implementation pattern + gentler curve (exchange-core source-verified).
- **Medium confidence:** Rust is the better *learning* language for this project (borrow checker as strict teacher), contingent on not hitting a borrow-checker wall that stalls Phase 0.
- **Unverified / gap:** recent Rust-in-HFT adoption claims and existence of mature Rust matching-engine references — web search was filtered. Re-verify before citing in any write-up.
- **Not a deciding factor:** raw performance at the 500k–1M target — both clear it.

## Recommendation + tiebreaker

**Lean Rust**, on the strength of the story axis and the data-race-safety property, *unless* the direct-benchmark-parity artifact is the #1 project priority — in which case Java is defensible and arguably better-evidenced.

**The tiebreaker the plan already prescribes (honor it):** at Phase 0, write a minimal order book in *both* — one weekend each — and pick the language you can fully explain under the "explain every line" rule. The language you can reason about under audit pressure is the one that survives the interview.

## Sources

- exchange-core `pom.xml` (Java 8, Maven, deps) — GitHub contents API, Aug 2026 (primary)
- exchange-core README (benchmark table + methodology) — `raw.githubusercontent.com`, Aug 2026 (primary, vendor self-reported)
- `rust-best-practices` skill chapters 3 & 9 (performance, pointers/thread-safety) — installed in-repo (Apollo, MIT)
- Project plan (`docs/PLAN.md`) constraints and the "explain every line" rule

## Gaps & open questions

- Recent Rust-in-HFT adoption: unverified (web filtered). Re-check on an open network.
- Existence of a mature, benchmark-disclosed Rust matching-engine reference: unverified.
- Your own tolerance for a borrow-checker wall in Phase 0: unknown — the "build in both" tiebreaker resolves it empirically.
