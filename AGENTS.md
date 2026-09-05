# AGENTS.md — Working context for coding agents

This file is the entry point for any agent (or human) working in this repo. It tells you what this project is, how the repo is organized, the rules that are non-negotiable, and where the real source-of-truth context lives.

> **Personal context lives OUTSIDE this repo** (this repo is public). The user's personal docs (`user-context.md`, `goals.md`, `finances/*`) and the college/skills source live in `~/Personal`. Do NOT copy personal details into this repo. Reference by path when needed.

---

## 1. What this project is

**Launchpad** is a flagship engineering project with three layers:

- **A — CORE (Rust):** a low-latency exchange core — order book, matching engine, risk/accounting, event-sourced journaling — benchmarked honestly against production references (exchange-core, ABIDES, LEAN).
- **B — SIM (Python):** an agent-based market simulator + experiment harness → a research paper with negative results included.
- **C — VENUE + RECORD:** a hosted paper-trading competition, plus a public 12-month decision log and quarterly write-ups.

The one-line pitch: **"I built the launchpad — the exchange, the simulation, and the competition — that traders trade on."**

The project's core property: **the difficulty is a measurable benchmark, not vibes.** Either the engine hits its ops/sec and latency targets or it doesn't. This is what makes it unfakeable and interview-proof. Everything in this repo serves that property — never compromise it for convenience.

## 2. Repo map

```
README.md                     # Public intro + status
AGENTS.md                     # You are here
.agents/skills/               # Agent skills (see §7)
docs/
  PLAN.md                     # Master plan: concept, rationale, milestones, risks, decision log
  architecture.md             # The 4-layer architecture + tech stack
  stack.md                    # Rust vs Java decision matrix (DECIDED: Rust, 2026-08-21)
  benchmarks.md               # Targets + the honest-methodology rule
  phases.md                   # Phase 0–7 with done-criteria and timeline
  guardrails.md               # Non-negotiable rules (educational framing, no fake results)
  research/                   # Deep-research notes on exchange-core, ABIDES, LEAN, IMC + source ledger
  record/                     # The 12-month record: decision log + weekly/quarterly templates
  paper.md                    # Research layer (B): candidate experiments + paper outline
  venue.md                    # Hosted competition (C): plan + requirements
  internships.md              # Internship playbook: the artifact kit + cold-email play
core/  sim/  venue/           # (code lands here from Phase 0 onward)
```

## 3. How to work here

1. **Read the phase you're working on first.** `docs/phases.md` defines what "done" means for the current phase and its stop condition. If a task isn't in the current phase, it's out of scope — flag it, don't build it.
2. **Every change ships with tests.** Unit + property-based tests are non-negotiable (see `docs/benchmarks.md` §"Testing & correctness discipline"). Order-book invariants: price-time priority, no crossed book, sum of fills = traded quantity.
3. **Honest numbers only.** Any performance claim must come with methodology (hardware, data mix, measurement method). A number without methodology is a claim; with methodology it's evidence. Never cherry-pick benchmarks, never claim a "profitable backtest" without full disclosure.
4. **Determinism is proven, not claimed.** Replay the journal, diff state. Same input → same output. There must be a test for this.
5. **Update the decision log.** Every design decision goes into `docs/record/decision-log.md` (date, decision, rationale, status). Weekly log entries are required — no missing weeks.
6. **Explain every line.** AI may write boilerplate, tests, and review code — but the user must be able to explain every line they ship. If you generate code, add comments explaining the *why*, and flag anything subtle that the user must understand before it's "theirs."
7. **Match conventions.** This repo follows the docs-first convention of the user's other projects (Laborious, Fin OS, Buddy): design decisions get written down before/while code gets written.

## 4. Tech stack (decided — see decision log)

- **CORE:** **Rust** (decided 2026-08-21) — memory safety + zero-cost abstraction; the modern HFT language; learning-depth axis. Alternative considered and deferred: Java (matches the exchange-core reference exactly). **See `docs/stack.md` — decided; re-open only if side-by-side benchmark parity becomes the top priority.**
- **SIM:** Python — simulator, experiments, data, logging.
- **VENUE:** Python/TS — paper accounts, leaderboard, competition engine.
- **Git + GitHub:** public repo from day 1 (the record starts immediately).

## 5. Guardrails (non-negotiable)

- **Educational framing only.** No managing other people's money, no personalized investment advice, no live automated trading until 18.
- **Paper trading only** in the venue. The real-money record is the user's own custodial Roth (governed by `~/Personal/docs/finances/investment-allocation-strategy.md`), never a pitch to others.
- **No fake results** — no invented benchmarks, no cherry-picked numbers, no fabricated track record.
- **No personal data in this repo.** It's public. See the note at the top.
- Full list: `docs/guardrails.md`.

## 6. Reference projects (the bar)

Study these; do not copy them wholesale. Research notes + source ledger in `docs/research/`.

| Reference | What to steal | What NOT to copy |
|---|---|---|
| exchange-core | Benchmark methodology, event-sourcing + snapshot design, order-book invariants, lock-free patterns | The Java/LMAX stack itself (if we go Rust) |
| ABIDES | Agent architecture, pairwise network latency model, ITCH/OUCH-style messaging, experiment design | The whole codebase — we build our own sim |
| LEAN | Modular plug-in architecture, backtest/live separation, data handling | Scope — LEAN is a full platform; we build a core + sim |
| IMC Prosperity | Competition format, leaderboard design, round structure | The closed platform — we host our own |

## 7. Skills available in this repo (`.agents/skills/`)

- **deep-research** — multi-angle research (steelman/skeptic/primary/recent), source triangulation, honest uncertainty. Use for any research task. Modes: lite/standard/deep/ultra.
- **questions** — adversarial interrogation of plans/ideas before committing. Use before locking in a design decision. Modes: lite/full/ultra.
- **college** — evidence-based extracurricular/application evaluation. Use when a decision affects the college story. (Invokes `/questions` + `/deep-research`.)
- **code-review-and-quality** — multi-axis code review before merging.
- **rust-best-practices** — idiomatic Rust guidance (borrowing, ownership, patterns). Use when writing/reviewing Rust.
- **rust-testing** — Rust testing discipline (unit, property, integration).

> **👉 Exact usage guidance — task → skill → mode — lives in [`docs/skills.md`](docs/skills.md).** Read it before starting any task that involves research, a design decision, Rust code, tests, or a merge.

These are the working skills. If a task needs a skill not listed here, search the ecosystem (`npx skills find <query>`) and propose adding it — don't silently install.

## 8. Where the real context lives (read-only references)

These are the user's source-of-truth docs. Read them when you need the *why* behind scheduling, budget, guardrails, or the college narrative. **Never copy their contents into this repo.**

- `~/Personal/docs/user-context.md` — single source of truth for the user's life/context
- `~/Personal/docs/goals.md` — junior/senior year goals (Launchpad is one project among several)
- `~/Personal/docs/finances/investment-allocation-strategy.md` — the governing money doc (Launchpad observes it, never overrides it)
- `~/Personal/college-extracurricular-opportunities/` — competition/opportunity research (Wharton, IMC, etc.)
- Sibling code repos: `~/labourious`, `~/Fin`, `~/Documents/Buddy` — same docs-first conventions

## 9. Current status

- **Phase:** 0 (Foundations) — kickoff Sept 2026
- **Open decisions:** benchmark targets finalization at Phase 2 (Rust-vs-Java and project name resolved 2026-08-21 — see `docs/stack.md` + `docs/record/decision-log.md`)
- **Next action:** Phase 0 — Rust crash course + order-book mechanics; study exchange-core + ABIDES + LEAN; minimal order book in Rust with unit tests

See `docs/phases.md` for the full phase table and `docs/record/decision-log.md` for the decision history.
