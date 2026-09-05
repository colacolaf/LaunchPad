# venue/

Paper-trading venue — the competition layer (C). **Empty by design** until
Phase 4+ (`docs/phases.md`); this placeholder keeps the repo map honest.

Python/TS, not Rust: VENUE talks to CORE over its JSON/WebSocket API and is
not a cargo workspace member.

What lands here (Phase 4+ — see `docs/venue.md`, `docs/architecture.md`):

- Paper-trading accounts (no real money, ever — `docs/guardrails.md`).
- Leaderboard + competition engine: rounds, scoring, published results.
- The hosted competition (spring 2027): ≥10 traders, ≥1 full round.

Done-criteria for the layer (from `docs/phases.md` Phase 4):
**a stranger can place an order without your help.**
