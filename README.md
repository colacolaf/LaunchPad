# Launchpad

> Build the market itself — a low-latency matching engine, an agent-based market simulator, and a hosted trading competition. Not another trading bot on top of the market: the venue traders trade on.

**Status:** Phase 0 — Foundations (Sept 2026) · **Last updated:** Sep 5, 2026

[![CI](https://github.com/colacolaf/LaunchPad/actions/workflows/ci.yml/badge.svg)](https://github.com/colacolaf/LaunchPad/actions/workflows/ci.yml) · [![Benchmarks](https://img.shields.io/badge/Benchmarks-pending-8A8A8A)]() · [![Decision log](https://img.shields.io/badge/Decision%20log-weekly-2E7D32)](docs/record/decision-log.md)

---

## What this is

Three layers, one project:

1. **A — The build (the rocket):** a matching engine + order book + risk/accounting + event-sourced journaling, benchmarked honestly against production references.
2. **B — The science (mission control):** original experiments on a custom agent-based market simulator (market impact, latency effects, agent behavior) → a paper with negative results included.
3. **C — The record (the launch):** a public, decision-logged track record over 12+ months, plus a hosted paper-trading competition.

The pitch in one line: **"I built the launchpad — the exchange, the simulation, and the competition — that traders trade on."**

## Why this project exists

The difficulty of this project is a **measurable benchmark, not vibes** — either the engine hits its ops/sec and latency targets or it doesn't. That makes it structurally unfakeable and interview-proof, and it's the strongest possible demonstration of sustained systems-level engineering. It powers both a college application and an internship portfolio. (See [`docs/PLAN.md`](docs/PLAN.md) for the full rationale; personal context lives outside this repo by design.)

## Repo layout

```
├─ .agents/skills/          # Agent skills (research, college, questions, code review, Rust)
├─ .github/workflows/       # CI: fmt / clippy / test / release smoke-bench
├─ core/                    # CORE (Rust) — order book, matching, risk, event sourcing
├─ bench/                   # Benchmark harness (Phase 2 — placeholder until then)
├─ sim/                     # SIM (Python) — agent simulator (Phase 5 — placeholder)
├─ venue/                   # VENUE — paper trading + competition (Phase 4+ — placeholder)
├─ docs/
│   ├─ PLAN.md              # Master build plan (the why + the what)
│   ├─ architecture.md      # CORE / SIM / VENUE / RECORD layers + tech stack
│   ├─ skills.md            # When to use each skill (task → skill → mode)
│   ├─ stack.md             # Rust vs Java decision matrix (DECIDED: Rust)
│   ├─ benchmarks.md        # Targets + honest methodology rule
│   ├─ phases.md            # Phase 0–7, done-criteria, timeline
│   ├─ guardrails.md        # Educational framing, no-fake-results rules
│   ├─ research/            # Deep-research notes on reference projects
│   ├─ record/              # The 12-month decision log + weekly/quarterly templates
│   ├─ paper.md             # The research layer (B): experiments → paper
│   ├─ venue.md             # The hosted competition (C)
│   └─ internships.md       # Internship playbook (the artifact kit)
├─ Cargo.toml               # Virtual workspace root: members, shared lints, resolver 3
├─ rustfmt.toml             # style_edition 2024 (only non-default config)
└─ LICENSE
```

## Reference bar (verified Aug 2026)

| Reference | What it is | The bar |
|---|---|---|
| [exchange-core](https://github.com/exchange-core/exchange-core) | Production-grade Java matching engine (LMAX Disruptor, lock-free) | ~150ns/match; 5M ops/sec single order book; 1ms wire-to-wire @ 1M+ ops/sec |
| [ABIDES](https://github.com/abides-sim/abides) ([arXiv:1904.12066](https://arxiv.org/abs/1904.12066)) | NYU academic agent-based market simulator | Tens of thousands of agents, configurable latencies, ITCH/OUCH-style messaging |
| [QuantConnect LEAN](https://github.com/QuantConnect/Lean) | Professional open-source algo engine | Full backtest + live-trading platform architecture |
| [IMC Prosperity](https://prosperity.imc.com/) | IMC's trading challenge (18,803 teams in 2026) | The competition model — we build the venue, peers trade on it |

## Getting started

**Build & test (Rust CORE):**
```bash
cargo build --workspace          # compile all crates
cargo test --workspace           # run unit tests
cargo fmt --all -- --check       # formatting gate (CI enforces)
cargo clippy --workspace --all-targets -- -D warnings   # lint gate (CI enforces)
```

The Rust workspace is scaffolded (virtual manifest, `core/` crate, shared lint
policy). Domain code — the minimal order book — is the next Phase 0 session
(see [`docs/TODO.md`](docs/TODO.md) §5). SIM (Python) and VENUE arrive in
Phases 4–5; their directories are placeholder-only until then.

CI runs on every push/PR: fmt, clippy (warnings are errors), tests, and a
release-profile smoke build. Benchmarks are pending by design until Phase 2 —
no numbers are published before a published methodology exists.

## License

[MIT](LICENSE) — permissive, use/study/fork freely. (The reference projects use Apache-2.0 and BSD-3; MIT keeps this repo maximally simple and consistent with the bundled agent skills.)
