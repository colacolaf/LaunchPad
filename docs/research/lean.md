# Research: QuantConnect LEAN

> **Repo:** github.com/QuantConnect/Lean · **Language:** C# (Python algorithms) · **Stars:** ~21,300 · **Verified:** Aug 21, 2026 (GitHub API)

## What it is

**LEAN** is the open-source algorithmic trading engine behind QuantConnect — an event-driven, professional-caliber platform for **backtesting and live trading** across multiple markets, with out-of-the-box alternative data support.

This is the **platform reference**: it shows what a complete algo-trading system looks like, so we can scope what Launchpad does and doesn't need.

## Why it's the bar

- Full **backtest + live-trading** architecture in one codebase.
- **Modular by design** — every component is pluggable and customizable; it ships with models for all major plug-in points (data feeds, brokerage, execution, risk, etc.).
- CLI (via Docker) for `project-create`, `research`, `backtest`, `optimize`, `live`.

## What to steal vs. what to skip

**Steal:**
- **The modular plug-in architecture:** separating data, brokerage, execution, and risk behind interfaces. Our CORE should keep its API surface clean so the SIM and VENUE can plug into it without coupling.
- **Backtest/live separation:** the same algorithm runs in backtest and live. For us: the same matching engine serves the SIM (backtest-ish) and the VENUE (live-ish paper trading).
- **The idea that research and production share one engine** — that's exactly our A→B→C design.

**Skip:**
- The scope. LEAN is a full trading platform (brokerages, options, forex, alternative data). We build a core + sim + a simple venue. Do not let LEAN's scope creep into our phases.

## Relevance to our phases

- **Phase 4 (API + venue v1):** borrow the "plug-in models" mindset — our API layer should be a thin, clean seam.
- **Architecture doc:** LEAN is the example of why CORE/SIM/VENUE stay decoupled behind interfaces.

## Source

- README (fetched from `raw.githubusercontent.com/QuantConnect/Lean/master/readme.md`, Aug 21, 2026)
- GitHub API repo metadata (Aug 21, 2026)
