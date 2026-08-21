# Guardrails

Non-negotiable rules for Launchpad. These are consistent with the user's broader finance docs (which govern the money side and are referenced, not copied).

## The hard rules

1. **Educational framing only.** No managing other people's money, no personalized investment advice, no live automated trading until 18.
2. **Paper trading only in the venue.** The venue runs on simulated accounts. The real-money record is the user's own long-term account (governed by an external strategy doc — `~/Personal/docs/finances/investment-allocation-strategy.md`), never a pitch to others.
3. **No fake results.** No invented benchmarks, no cherry-picked numbers, no fabricated track record, no "profitable backtest" claims without methodology.
4. **Methodology ships with every number.** A benchmark without hardware/data-mix/measurement-method is a draft, not a result (`docs/benchmarks.md`).
5. **No personal data in this repo.** This repo is public. Personal context lives outside it and is referenced by path only (`AGENTS.md` §8).
6. **The governing money doc stays in charge.** Launchpad observes the external investment strategy; it never overrides it.

## Guardrails for the narrative

- Never claim results you don't have. The record and the benchmarks are the ceiling of what you can say — and they're already impressive.
- Never present the paper as finished research before it exists. Planned ≠ completed.
- Courses stay in the background of the story: they explain the vocabulary, they don't carry the weight.

## Compliance notes (verify before acting)

- Unpaid work for a business can implicate state/federal labor laws for minors. The safe framing is **independent project collaboration** (you build, they give feedback/reference), not "employment." Involve parents in any arrangement. Verify your state's rules before committing.
- Broker APIs (Alpaca, IB, etc.) require 18+. No live automated trading until then.
- IMC Prosperity prizes are university-restricted — verify eligibility before relying on them.

## Anti-patterns (never these)

- virattt/ai-hedge-fund-style "AI hedge fund" repos that don't actually trade — the README literally says the system doesn't make trades. Launchpad is the opposite: real, measured, honest.
- Any backtest or benchmark that hides its assumptions (leakage, survivorship bias, transaction costs).
- Publishing a benchmark number without its methodology.
