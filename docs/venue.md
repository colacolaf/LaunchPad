# The Venue (C) — the hosted competition

> The launch: a real trading competition on your own venue. Modeled on IMC Prosperity's round/leaderboard structure (see `docs/research/imc-prosperity.md`).

## The goal

Host a paper-trading competition on the Launchpad venue, spring 2027:
- **≥10 traders**
- **≥1 full round**, results published
- Proof people used it (the artifact that makes the whole project real)

## Requirements (from Phase 4 + 6)

- **Paper accounts** — simulated money, no real funds, educational framing only (guardrails).
- **API** — a stranger can place an order without your help (Phase 4 done-criteria).
- **Leaderboard** — transparent scoring, published results.
- **Competition engine** — rounds, start/end, scoring rules.

## Competition design (borrowed from IMC Prosperity)

- **Rounds:** a handful of rounds, each with a defined window.
- **Mix:** algorithmic + manual participation (Prosperity's model — code and manual trading both count).
- **Leaderboard:** transparent, updated, final results published.
- **Incentive:** prizes optional and must respect the educational framing (no real-money prizes without careful compliance review; recognition/bragging-rights are the safe default).

## Timeline decision (DEFERRED to Phase 4/5)

- **Option A:** April–May 2027 — aligned with IMC Prosperity 2027 as optional external validation (enter their venue as a trader while hosting yours).
- **Option B:** earlier standalone run in March.
- **Decision 2026-08-21:** defer to Phase 4/5 planning; revisit once the venue exists. Log the call then in `docs/record/decision-log.md`.

## Recruitment

- Classmates + the team competition network (warm beats cold).
- Target ≥10 committed traders; over-recruit for drop-off.

## Compliance

- Paper trading only. Educational framing. No real money, no personalized advice, no live automated trading (guardrails).
- If prizes are offered, verify the rules/labor/age constraints first — the safe default is recognition-only.

## Status

- [ ] API + CLI demo (Phase 4)
- [ ] Paper accounts + leaderboard (Phase 6)
- [ ] ≥10 traders, ≥1 full round, results published (Phase 6)
