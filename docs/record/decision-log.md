# Decision log

> Append-only. One line per decision/session, ~2 min. Every design choice, every paper trade, every miss. The record starts today.

| Date | Decision | Rationale | Status |
|---|---|---|---|
| 2026-08-21 | Project: Launchpad — matching engine + simulator + hosted competition | Most AI-proof, interview-proof, dual-purpose (portfolio + internships) | Adopted |
| 2026-08-21 | Stack: Rust core + Python sim | Modern HFT language, safety + performance, strong AI-assist learning path | Confirm at Phase 0 |
| 2026-08-21 | Public repo from day 1; decision log weekly; quarterly write-ups | The 12-month record (C) is a core deliverable, not a nice-to-have | Adopted |
| 2026-08-21 | Repo scaffolded: AGENTS.md, docs/ (plan, architecture, stack, benchmarks, phases, guardrails, research, record, paper, venue, internships) | Self-contained context for future coding agents; public-safe (no personal data) | Adopted |
| 2026-08-21 | Skills installed in-repo: deep-research, college, questions (personal) + code-review-and-quality, rust-best-practices, rust-testing (ecosystem) | The working skill set for research, planning, and code quality | Adopted |
| — | Benchmark targets final (v2 = 500k ops/sec) | Calibrated to exchange-core reference | Confirm at Phase 2 |

## Template row

| YYYY-MM-DD | [What was decided/done] | [Why — one line] | [Adopted / Confirm at / Rejected] |

## How to add an entry

- Date = today.
- Decision = the thing you chose or did (one line).
- Rationale = why (one line, honest).
- Status = Adopted / Confirm at [phase] / Rejected / Deferred.
- Newest entries go on top of the table (or keep chronological — pick one and stay consistent; chronological is easier to audit).
