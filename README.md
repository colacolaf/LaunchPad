# Launchpad

> Build the market itself — a low-latency matching engine, an agent-based market simulator, and a hosted trading competition. Not another trading bot on top of the market: the venue traders trade on.

**Status:** Planning → Phase 0 kickoff (Sept 2026) · **Last updated:** Aug 21, 2026

[![CI](https://img.shields.io/badge/CI-pending-8A8A8A)]() · [![Benchmarks](https://img.shields.io/badge/Benchmarks-pending-8A8A8A)]() · [![Decision log](https://img.shields.io/badge/Decision%20log-weekly-2E7D32)](docs/record/decision-log.md)

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
├─ docs/
│   ├─ PLAN.md              # Master build plan (the why + the what)
│   ├─ architecture.md      # CORE / SIM / VENUE / RECORD layers + tech stack
│   ├─ skills.md            # When to use each skill (task → skill → mode)
│   ├─ stack.md             # Rust vs Java decision matrix (open question)
│   ├─ benchmarks.md        # Targets + honest methodology rule
│   ├─ phases.md            # Phase 0–7, done-criteria, timeline
│   ├─ guardrails.md        # Educational framing, no-fake-results rules
│   ├─ research/            # Deep-research notes on reference projects
│   ├─ record/              # The 12-month decision log + weekly/quarterly templates
│   ├─ paper.md             # The research layer (B): experiments → paper
│   ├─ venue.md             # The hosted competition (C)
│   └─ internships.md       # Internship playbook (the artifact kit)
└─ core/  sim/  venue/      # (code lands here from Phase 0 onward)
```

## Reference bar (verified Aug 2026)

| Reference | What it is | The bar |
|---|---|---|
| [exchange-core](https://github.com/exchange-core/exchange-core) | Production-grade Java matching engine (LMAX Disruptor, lock-free) | ~150ns/match; 5M ops/sec single order book; 1ms wire-to-wire @ 1M+ ops/sec |
| [ABIDES](https://github.com/abides-sim/abides) ([arXiv:1904.12066](https://arxiv.org/abs/1904.12066)) | NYU academic agent-based market simulator | Tens of thousands of agents, configurable latencies, ITCH/OUCH-style messaging |
| [QuantConnect LEAN](https://github.com/QuantConnect/Lean) | Professional open-source algo engine | Full backtest + live-trading platform architecture |
| [IMC Prosperity](https://prosperity.imc.com/) | IMC's trading challenge (18,803 teams in 2026) | The competition model — we build the venue, peers trade on it |

## Getting started

Nothing to run yet — this is the planning/context repo. Phase 0 (Sept 2026) scaffolds the Rust core. See [`docs/phases.md`](docs/phases.md).

## License

TBD (decide before first public commit).
