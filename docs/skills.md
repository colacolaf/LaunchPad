# Skills — when to use each one

> The working skills live in `.agents/skills/`. This is the decision guide: **what task → which skill → which mode**. If a task needs a skill not listed here, search the ecosystem (`npx skills find <query>`) and propose adding it — don't silently install.

## The inventory

| Skill | Purpose (one line) | Source |
|---|---|---|
| `deep-research` | Multi-angle research with source triangulation and honest uncertainty | Personal (`~/Personal/.agents/skills/`) |
| `questions` | Adversarial interrogation of a plan/idea *before* committing | Personal |
| `college` | Evidence-based evaluation of how a decision affects the portfolio/application story | Personal |
| `code-review-and-quality` | Multi-axis code review before merge | Ecosystem (global) |
| `rust-best-practices` | Idiomatic Rust: ownership, error handling, performance, docs | Ecosystem (Apollo) |
| `rust-testing` | Rust test patterns: unit, integration, async, property-based, coverage | Ecosystem (ECC) |

## Decision matrix — "I'm about to…"

| Task | Skill | Mode | Notes |
|---|---|---|---|
| Research a reference project, a claim, a tool choice, or an open question | `deep-research` | Standard for most; **Ultra** for high-stakes (stack choice, benchmark targets, paper claims) | Always multi-angle; never for trivial single-fact questions |
| Lock in a design decision (stack, architecture, phase scope, competition timing) | `questions` | Full for most; **Ultra** before irreversible calls | Run *before* committing, not after |
| Check whether a decision strengthens or weakens the application story | `college` | Quick for a single activity; Full/Ultra for profile-level calls | Invokes `questions` + `deep-research` — let it |
| Write Rust code (new functions, modules, the order book, the matching engine) | `rust-best-practices` | Always | Idiomatic ownership/borrowing, `Result` error handling, performance |
| Write Rust tests (unit, integration, property-based, coverage) | `rust-testing` | Always | Property tests are mandatory for order-book invariants |
| Review code before merging (yours, an agent's, or a human's) | `code-review-and-quality` | Always — every merge | Five axes: correctness, readability, architecture, security, performance |
| Write or update docs | (none required) | — | Follow the conventions in `AGENTS.md`; keep docs-first |
| Debug a failing test or bug | (none required) | — | Use the systematic-debugging approach; `rust-testing` + `code-review-and-quality` help |
| Decide the weekly plan / next session goal | (none required) | — | One-line goal from `docs/phases.md` |

## How the skills chain (the workflows)

1. **Design decision** → `questions` (interrogate) → `deep-research` (fill evidence gaps) → `college` (check the portfolio angle, if it matters) → log in `docs/record/decision-log.md`.
2. **Research task** → `deep-research` (steelman / skeptic / primary / recent) → write notes into `docs/research/` with a source ledger.
3. **Code task** → `rust-best-practices` (write) + `rust-testing` (test) → `code-review-and-quality` (review before merge) → update the decision log if a design choice changed.
4. **Phase completion** → tests + benchmarks green → `code-review-and-quality` → write the weekly log entry → update `docs/record/decision-log.md`.

## Mode quick reference

- **`deep-research`:** Lite (simple fact check — usually skip the skill), Standard (most research), Deep (high-stakes / contested), Ultra (academic-grade, counter-search every major claim).
- **`questions`:** Lite (3–5 questions, verbal verdict), Full (nitpick + counter-search), Ultra (adversarial, assume broken until proven otherwise, structured report).
- **`college`:** Quick (one activity), Full (profile/list), Ultra (high-stakes, explicitly requested).

## Rules

1. **Don't use a skill for a trivial task.** A one-line fact check doesn't need `deep-research`; a one-line fix doesn't need a full `questions` interrogation. Scale the mode to the stakes.
2. **`questions` runs BEFORE committing**, `code-review-and-quality` runs BEFORE merging. Both are gates, not afterthoughts.
3. **`college` is the only skill that pulls in others automatically** — let it run its handoff to `questions` and `deep-research`.
4. **Skills are advisory, not a substitute for judgment.** The "explain every line" rule still applies to anything generated with a skill.
5. **If a needed skill is missing**, propose adding it (search + install + add to this table) rather than improvising.

## Adding a skill

```bash
npx skills find <query>       # search
npx skills add <owner/repo@skill> -y   # install (note: this repo's network blocks github.com git — use the raw/API workaround if needed)
```

Then: add a row to the inventory table + the decision matrix, and note it in `docs/record/decision-log.md`.
