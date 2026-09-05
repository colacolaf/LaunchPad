# Launchpad — Build Plan

> The flagship project. The finance version of building a rocket ship and launching it — you build the market itself, not another trading bot on top of it.

**Last updated:** Aug 21, 2026 · **Status:** Planning → Phase 0 kickoff Sept 2026
**Skills used:** `/deep-research` ultra (project research), `/college` ultra (admissions verdict), `/questions` emulated audit
**Source context:** external (referenced, not copied — see `AGENTS.md` §8)
**Code home:** this repo. This doc is the plan; the code lives here.

---

## TL;DR — The verdict

Build **Launchpad**: a low-latency matching engine + exchange core, then an agent-based market simulator, then host a real trading competition on your own venue. Three layers, one project:

- **A — The build (the rocket):** matching engine + order book + risk/accounting + event-sourced journaling, benchmarked honestly against production references.
- **B — The science (mission control):** run original experiments on your simulator (market impact, latency effects, agent behavior) → publish a paper with negative results included.
- **C — The record (the launch):** a public, decision-logged track record over 12+ months. Start the clock now.

**Why this one wins:** it is the most AI-proof project in finance — the difficulty is a measurable benchmark, not vibes, so it's structurally unfakeable and interview-proof. That property is exactly what makes a strong engineering portfolio and a cold-email internship pitch work.

**Why it also wins for internships:** "I built a matching engine that processes X orders/sec and ran a trading competition on it" converts "hire me" (no) into "let me build you a tool for free" (sometimes yes). See `docs/internships.md`.

## 1. The concept

Real production references set the bar (all verified Aug 2026 — see `docs/research/`):

| Reference | What it is | The bar |
|---|---|---|
| exchange-core (github.com/exchange-core/exchange-core) | Production-grade Java matching engine, LMAX Disruptor, lock-free | ~150ns per match; 5M ops/sec single order book; 1ms wire-to-wire @ 1M+ ops/sec |
| ABIDES (github.com/abides-sim/abides, arXiv:1904.12066) | NYU academic agent-based market simulator | Tens of thousands of agents, configurable network latencies, NASDAQ ITCH/OUCH-style messaging |
| QuantConnect LEAN (github.com/QuantConnect/Lean) | Professional open-source algo engine | Full backtest + live-trading platform architecture |
| IMC Prosperity (prosperity.imc.com) | IMC's famous trading challenge (18,803 teams in 2026) | The competition model — you build the venue, peers trade on it |

The pitch in one line: **"I built the launchpad — the exchange, the simulation, and the competition — that traders trade on."**

## 2. Why this is the one

| Property | Why it matters |
|---|---|
| AI-proof | An AI agent generates a naive order book that fails at 1/100th of the benchmark. Hitting the real numbers requires genuine systems skill. AI raises the floor; the ceiling is yours. |
| Unfakeable | Either the engine hits the ops/sec and latency targets or it doesn't. No invented results possible. |
| Interview-proof | You can talk about it for 10 minutes without AI-babble. |
| Rare | Tens of thousands of applicants have AI trading bots; almost none have built the exchange. |
| Dual-purpose | Same artifacts power the portfolio story AND the internship pitch. |
| Sustained build | A 6+ month sustained build is the strongest possible counter-evidence to any "inconsistent effort" narrative. The project is the proof the discipline is real. |

**The anti-pattern (never this):** virattt/ai-hedge-fund (the most-starred "AI hedge fund" repo — README literally says "the system does not actually make any trades"). That's what everyone submits. Launchpad is its opposite: real, measured, honest.

## 3. Architecture

```
LAUNCHPAD
├─ CORE (Rust) — the exchange
│   ├─ Order book (price-time priority; limit / GTC / IOC / FOK / market)
│   ├─ Matching engine (continuous double auction)
│   ├─ Risk & accounting (balances, position limits, maker/taker fees, margin modes)
│   ├─ Event sourcing (disk journal, snapshots, deterministic replay)
│   └─ API layer (JSON/WebSocket for the venue; ITCH/OUCH-style feed for the simulator)
├─ SIM (Python) — the science
│   ├─ Agent-based market simulator (ABIDES-style: thousands of agents, configurable latencies)
│   ├─ Experiment harness (market impact, latency sensitivity, agent behavior)
│   └─ Research notebooks → the paper (B)
├─ VENUE (Python/TS) — the launch
│   ├─ Paper-trading accounts, leaderboard, competition engine
│   └─ The hosted competition (spring 2027)
└─ RECORD (repo + docs) — the proof
    ├─ Public README with honest benchmark results + CI badge
    ├─ Decision log (every trade/decision, weekly)
    └─ Quarterly write-ups incl. the bad quarters
```

**Tech stack** (recommended, confirm in the decision log — see `docs/stack.md`):

- **Rust for the core.** Why: memory safety + zero-cost abstraction = the modern HFT language; strongest AI-assist support for beginners. Alternative: Java (matches exchange-core reference exactly, LMAX Disruptor) or C++ (industry standard, highest difficulty). **Decision: Rust core + Python sim** — clean split, hot path in Rust, research in Python.
- **Python for the simulator**, experiments, data, logging.
- **Git + GitHub** (public repo from day 1 — the record starts immediately).

## 4. What you'll learn (the point)

- Systems programming (Rust), concurrency, lock-free data structures, memory layout
- Event sourcing, deterministic replay, snapshot/restore
- Market microstructure: order books, price-time priority, bid-ask dynamics, latency, market impact
- Statistics & honest backtesting (leakage, survivorship bias, transaction costs)
- Benchmarking discipline: percentiles, not averages; controlled conditions
- Running something real with real people on it (the competition)

Math needed: mostly not calculus — order-book mechanics and discrete event simulation are logic/CS-heavy.

## 5. Milestones & timeline

Budget: ~8 hrs/week, weekends only (deep-focus sessions + Sunday afternoon; other commitments keep their own slots — this is the flexible layer).

| Phase | When (2026–27) | Goal | Deliverable / Done-criteria | Stop condition |
|---|---|---|---|---|
| 0. Foundations | Sep | Rust crash course (AI agent as tutor) + order-book mechanics; study exchange-core + ABIDES + LEAN | Minimal order book in Rust with unit tests | When you can explain price-time priority in 3 sentences and your tests pass |
| 1. Matching engine v1 | Oct | Correct, functional matching | Order types (limit/GTC/IOC/FOK/market), unit + property tests passing | All correctness invariants green |
| 2. Benchmark & optimize | Nov–Dec | Measured performance | Benchmark harness: v1: 50k ops/sec → v2: 500k → stretch: 1M+ ops/sec; latency percentiles (p50/p99) | Diminishing returns > 2 sessions at a plateau; note the number honestly |
| 3. Risk + event sourcing | Jan | Production-grade depth | Balances, position limits, fees; journal + snapshot + replay; replay determinism test | Replay produces identical state — proven, not claimed |
| 4. API + venue v1 | Feb | A place people can trade | JSON/WebSocket API, CLI demo, paper accounts | A stranger can place an order without your help |
| 5. Simulator | Mar | The science engine | ABIDES-style agent simulator (thousands of agents, configurable latency), 2+ experiments run | Experiments produce reproducible results |
| 6. Host the competition | Apr–May | The launch | Recruit participants; leaderboard; run it; write-up | ≥10 traders, ≥1 full round, results published |
| 7. Paper + record + internships | Summer 2027 | The payoff | Research paper draft (B) with negative results; 12-month record write-up (C); internship push | Paper has reproducible code; record has no missing weeks |

**Threading with existing commitments:** a team investment competition runs Sept 28 – Dec 4 (the market-microstructure knowledge strengthens the report — keep it the priority in Oct–Nov; Launchpad is the flexible layer). IMC Prosperity 2027 (~April) is optional external validation — enter as a trader on their venue while hosting yours. Participation is open; prizes are university-restricted (verify before relying).

**Weekly rhythm** (one window, one task):
- Sat block 1: core build (Rust) · Sat block 2: sim/research or write-up · Sun afternoon: tests, review, decision-log entry, next-week plan
- Rule: never open-ended sessions. Every session has a one-line goal from the phase table.
- Rule: scope changes get a 24-hour cool-off before adoption.

## 6. Benchmark targets (honest metrics)

| Metric | v1 (correct) | v2 (optimized) | Stretch | Reference |
|---|---|---|---|---|
| Throughput | 50k ops/sec | 500k ops/sec | 1M+ ops/sec | exchange-core: 5M (10-yr-old hardware) |
| Match latency (p99) | < 1ms | < 100µs | < 10µs | exchange-core: ~150ns/match |
| Replay determinism | — | identical state | — | required |
| Simulator agents | 1k | 10k | 50k+ | ABIDES: tens of thousands |

**The rule that makes it honest:** publish the benchmark methodology (hardware, data mix, measurement method) with the numbers. A number without methodology is a claim; with methodology it's evidence. This is the anti-vibe-coding rule that makes it audit-proof. Full details: `docs/benchmarks.md`.

## 7. Testing & correctness discipline (non-negotiable)

- Every component ships with tests. Unit + property-based (order-book invariants: price-time priority, no crossed book, sum of fills = traded quantity).
- Determinism is proven, not claimed: replay the journal, diff state. Same input → same output.
- AI-assist rule: AI may write boilerplate, tests, and review code — but you must be able to explain every line you ship. "If AI wrote it and you can't explain it, it's not yours" — and the interview will find out.
- No fake green: CI runs tests + benchmarks; the README badge is live, not a screenshot.
- Negative results are published (in the paper and the record). Honest failure is the rare signal; that's the point.

## 8. The research layer (B) — turning the simulator into a paper

The simulator isn't a toy — it's an instrument. Candidate experiments (pick 2–3, preregister, run, report honestly):

- **Latency effects on spread:** vary simulator latency, measure equilibrium spread/bid-ask bounce. Does faster matching tighten spreads? By how much?
- **Market impact of order size:** how does impact scale with order size vs. book depth?
- **Agent behavior:** naive agents (random/trend-following) vs. market-making agents — who earns, who loses, and why (ties to Barber & Odean ~97% retail-trader-loss finding).
- **Maker/taker fee effects** on liquidity provision.

Publishing route: paper draft → high-school journals/competitions, the team competition report, and the internship portfolio. Include the negative results. Data: free tiers (SEC EDGAR XBRL APIs are free/no-key — verified; FRED; yfinance) if real-market grounding is needed. Full detail: `docs/paper.md`.

## 9. The record layer (C) — the 12-month clock starts NOW

- Public repo from day 1 with a decision log: every design choice, every trade (paper), every session, every miss. One line per session, 2 min.
- Quarterly write-ups (Nov, Feb, May, Aug): what worked, what failed, what the benchmarks say. Include the bad quarters — that's what makes it unfakeable.
- The record is the essay material. "12 months of logged decisions" cannot be AI-generated.
- Capstone: connect to the actual long-term investment strategy (the boring money runs on the same discipline the record documents). No live automated trading until 18 (broker APIs require 18+ — verify before relying).

Templates live in `docs/record/`.

## 10. Definition of done (junior year)

Launchpad "shipped" =

- Matching engine + order book passing correctness tests
- Benchmark harness with published methodology and honest numbers (≥500k ops/sec target)
- Risk/accounting + event sourcing with proven replay determinism
- Paper-trading venue with API, used by ≥1 person who isn't you
- Competition hosted (≥10 traders, results published)
- 1–2 simulator experiments documented
- Decision log with zero missing weeks
- Paper draft (B) started, negative results included

Senior-year stretch: full research paper, deeper benchmark work, open-source contribution to exchange-core/QuantLib/LEAN.

## 11. Internship playbook (the "build it for free" lever)

The core insight: nobody hires a 16-year-old. But some firms will accept a free, impressive, self-contained build. So you never pitch "I want an internship" — you pitch "I built X; I'll build you Y for free as a case study." The project converts employment friction into a no-friction offer (a tool appears in their inbox).

**What you're selling (the artifact kit):**
- GitHub repo — README with benchmarks + methodology + CI badge
- 2-minute demo video (matching engine running, latency numbers live)
- One-page write-up (what/why/results, honest limits)
- The hosted competition (proof people used it)
- (Later) the paper draft

**Target list** (cold email / LinkedIn, in order of realism): local RIAs / independent wealth managers / financial advisors · CPA and accounting firms · small fintech / finance startups · local credit unions, community banks, insurance agencies · family offices (via any warm connection) · professors / alumni networks / parents' network (warm beats cold) · the team competition advisor + team parents.

**Realistic expectations:** expect a ~5–10% reply rate and mostly "no"s — liability rules and HR friction kill most unpaid-minor setups regardless of skill. Every "no" is free market research: ask why and adapt the offer. Target 1–3 wins by summer 2027. A "win" can be a 2-week project collaboration, a reference, a mentor, or a real unpaid internship.

**Compliance note (flag to verify):** unpaid work for a business can still implicate state/federal labor laws for minors. The safe framing is independent project collaboration (you build, they give feedback/reference), not "employment," and involve parents in any arrangement. Verify your state's rules before committing to anything.

Full playbook: `docs/internships.md`.

## 12. Guardrails (non-negotiable)

- Educational framing only. No managing other people's money, no personalized investment advice, no live automated trading until 18.
- Paper trading only in the venue; the real-money record is the user's own long-term account (governed by an external strategy doc), never a pitch to others.
- No fake results, no cherry-picked benchmarks, no "profitable backtest" claims without methodology.
- The doc that governs the money side stays external (in the user's finance docs); Launchpad observes it, doesn't override it.

Full list: `docs/guardrails.md`.

## 13. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Scope creep (this project is huge) | Phases are the contract; stop conditions defined; one window one task; 24-hour cool-off on scope changes |
| AI does the work, you learn nothing | The "explain every line" rule; interviews will audit you |
| Time pressure vs. team competition (Dec 4) | Launchpad is the flexible layer Oct–Nov; benchmarks don't have to be final by December |
| Boredom/plateau | Every phase ends in a visible artifact (test pass, benchmark jump, hosted round) — streak-friendly wins |
| Plateaus in benchmark work | Plateau rule: 2 sessions, then publish the honest number and move to the next phase — the record rewards honesty, not fake progress |
| Internship "no"s demoralize | Treat as market research; pre-commit to 20 sends before judging the approach |

## 14. Decision log (fill as you go)

| Date | Decision | Rationale | Status |
|---|---|---|---|
| 2026-08-21 | Project: Launchpad — matching engine + simulator + hosted competition | Most AI-proof, interview-proof, dual-purpose (portfolio + internships) | Adopted |
| 2026-08-21 | Stack: Rust core + Python sim — committed (write-both-then-pick rule dropped) | Learning-depth is the #1 axis; building from scratch in Rust deepens learning; deep-research confirmed the recommendation; Java comparison skipped. | Adopted |
| 2026-08-21 | Public repo from day 1; decision log weekly; quarterly write-ups | The 12-month record (C) is a core deliverable, not a nice-to-have | Adopted |
| — | Benchmark targets final (v2 = 500k ops/sec) | Calibrated to exchange-core reference | Confirm at Phase 2 |

## 15. Open questions

> **Resolved 2026-08-21 (see `docs/record/decision-log.md`):**
> - **Name:** "Launchpad" (lowercase p) — confirmed as the project name.
> - **Rust vs Java:** **Rust** (Python for SIM) — decided on the learning-depth axis; building from scratch deepens the learning. See `docs/stack.md` + `docs/research/rust-vs-java.md`.
> - **Competition timing:** **deferred** to Phase 4/5 planning. See `docs/venue.md`.
> - **Paper venue (B):** **SSRN-style preprint** as primary; 2–3 additional targets researched at Phase 7. See `docs/paper.md`.
>
> Remaining open (revisit as flagged):

- **Build-off tiebreaker:** whether to still build a minimal Java order book for a side-by-side benchmark, now that Rust is chosen. Optional — only worth it if benchmark parity becomes a top priority.
- **Repo/repo-name casing:** docs use "Launchpad"; the GitHub remote is currently camelCase "LaunchPad" (`github.com/colacolaf/LaunchPad`). Rename the remote separately if desired.
- **License:** TBD before any external dependency or contribution (MIT or Apache-2.0; exchange-core uses Apache-2.0).

## 16. Framing (the narrative spine)

The spine in one sentence: **"I don't trust black boxes — I build the machinery."** Every beat of the story serves that line. If a detail doesn't serve it, cut it.

The arc (4 beats):
1. **Curiosity.** Studied markets (financial markets, corporate finance, PE/VC, AI-in-finance) and built an AI research firm that stress-tests investment ideas from multiple angles before trusting any of them.
2. **The wall.** To test strategies I needed a backtest — and every backtesting library was a black box: I couldn't see the matching, the order book, the latency and liquidity assumptions hiding underneath. I refused to trust results I couldn't inspect.
3. **The build.** So I built the market myself: a matching engine + order book + risk/accounting + event-sourced exchange core (benchmarked honestly), then an agent-based simulator to run experiments, then hosted a paper-trading competition on my own venue.
4. **The proof.** 12 months of public decision logs with the bad quarters included, a research paper with negative results, and my own long-term money running on the same discipline — the framework doesn't just work in code, it works where it hurts.

**Guardrails for the narrative:** never claim results you don't have (the record and the benchmarks are the ceiling of what you can say). Never present the paper as finished research before it exists (planned ≠ completed). Courses stay in the background — they explain the vocabulary, they don't carry the weight.

> This doc is the plan. The code repo is the evidence. The record starts today.
