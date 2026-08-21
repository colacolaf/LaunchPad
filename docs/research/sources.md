# Source ledger — reference research

All sources verified **Aug 21, 2026**. Ledger format follows the `/deep-research` skill's source ledger.

| ID | Claim supported | Source / URL | Source type | Retrieved | Confidence | Limitation |
|---|---|---|---|---|---|---|
| R-001 | exchange-core exists, public, Java, 2,602★, Apache-2.0, LMAX Disruptor/lock-free | api.github.com/repos/exchange-core/exchange-core | Primary (GitHub API) | 2026-08-21 | High | Stars are point-in-time |
| R-002 | exchange-core benchmark table + methodology (data mix, hardware, percentile shape) | raw.githubusercontent.com/exchange-core/exchange-core/master/README.md | Primary (project README) | 2026-08-21 | High | Vendor self-reported; methodology is exactly what we copy, not the numbers |
| R-003 | exchange-core features: event sourcing, journal+snapshots, LZ4, no-floating-point, maker/taker fees, IOC/GTC/FOK-B, risk modes | same as R-002 | Primary | 2026-08-21 | High | — |
| R-004 | ABIDES exists, public, Python, 560★ | api.github.com/repos/abides-sim/abides | Primary (GitHub API) | 2026-08-21 | High | — |
| R-005 | ABIDES: tens of thousands of agents, pairwise latencies, ITCH/OUCH messaging, market-impact experiment | arxiv.org/abs/1904.12066 (abstract) + repo README | Primary (paper + README) | 2026-08-21 | High | Abstract-level only; full paper not re-fetched |
| R-006 | LEAN exists, public, C#, 21,287★, event-driven backtest+live platform | api.github.com/repos/QuantConnect/Lean | Primary (GitHub API) | 2026-08-21 | High | — |
| R-007 | LEAN: modular plug-in architecture, CLI, Docker-based local runs | raw.githubusercontent.com/QuantConnect/Lean/master/readme.md | Primary (project README) | 2026-08-21 | High | README is marketing-ish; architecture claims not code-verified |
| R-008 | IMC Prosperity 4: 18,803 teams, 5 rounds, $50K prize pool, university students, 1–2 hrs/day | prosperity.imc.com landing page | Primary (official site) | 2026-08-21 | High | Landing-page level; rules/eligibility not fully re-read |
| R-009 | IMC Prosperity prizes university-restricted | plan citation; flagged for verification | Secondary | — | Medium | Needs direct rules check before relying |

## Verification notes

- **Network caveat:** this environment's network is behind a web filter that blocks `github.com` (HTML + git) but allows `api.github.com` and `raw.githubusercontent.com`. All GitHub data was fetched via the API/raw hosts. Re-verify on an unfiltered network before publishing anything that cites a GitHub HTML page.
- **exchange-core numbers are the vendor's own benchmarks.** We copy the *methodology* as our discipline; we do not claim their numbers as ours.
- **IMC prize eligibility** must be re-verified against the official rules before any reliance (the plan itself flags this).
