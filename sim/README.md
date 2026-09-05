# sim/

Agent-based market simulator — the research layer (B). **Empty by design**
until Phase 5 (`docs/phases.md`); this placeholder keeps the repo map honest.

Python, not Rust: SIM is deliberately a different toolchain from CORE
(see `docs/stack.md`). It will not be a cargo workspace member.

What lands here (Phase 5 — see `docs/architecture.md`, `docs/paper.md`):

- ABIDES-style simulator: thousands of agents (random, trend-following,
  market-making) + an exchange agent speaking ITCH/OUCH-style messages to
  the CORE layer's feed.
- Configurable **pairwise network latencies** per agent↔exchange pair.
- Experiment harness: run a scenario N times with a fixed seed; reproducible
  results feed the paper (market impact, latency effects, fee effects,
  agent behavior — negative results included).

Toolchain setup (per `docs/TODO.md` §1): pyenv/venv when Phase 5 nears.
`python3 --version` on this machine: 3.14.3 (verified 2026-09-05).
