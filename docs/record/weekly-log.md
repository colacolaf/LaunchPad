# Weekly log

> One entry per week, no missing weeks. Sunday afternoon: write the entry + plan next week. If a week is missed, backfill honestly ("missed, backfilled") — never fake an entry.

## 2026

### Week of 2026-09-05 (Phase 0 kickoff)

- **Built:** Rust workspace scaffold — virtual cargo workspace (resolver 3), `launchpad-core` crate (edition 2024, zero deps, smoke test), shared `[workspace.lints]` (unsafe forbid, missing_docs warn), rustfmt style_edition 2024, placeholder `bench/`/`sim/`/`venue/`, 4-job GitHub Actions CI (fmt / clippy `-D warnings` / test / release smoke), README getting-started + live CI badge. All four gates verified locally before commit.
- **Built (3rd session, same week):** the Phase 0 order book (`core/src/book.rs`) — `OrderBook` with place/cancel/move_order, best-bid/best-ask accessors, limit/IOC/FOK/market semantics, `Fill` events at maker prices; 28 unit tests + the four property invariants (price-time, no-cross, per-side conservation, determinism) via `proptest`. **The property suite earned its keep immediately:** it caught a real index-leak bug (fully-filled makers left in the id→locator index) plus a conservation-accounting subtlety (fills consume from both sides). First real CI run #33991924373 all green — badge proven.
- **Built (4th session, same week):** the TODO §2/§3 learning curriculum in `docs/learning/` — a Socratic walkthrough of the real code (8 lessons: split borrows, Copy semantics, the strict parser, enum modeling, the failure ladder, visibility, traits incl. `Box<dyn Iterator>`, and the full microstructure story), a 28-question closed-book self-test with the §9 exit-gate exercise and rubric, and a cited answer key. Every claim empirically verified by temporary audit tests — which caught **two errors in the tutor's own worked examples** (a fill-arithmetic slip and a wrong "remainder rests" claim; the corrected extension question — *why place can never rest into a lock* — is now stronger than the original). CI re-proven on the order-book push (run #34047751858, all four jobs green).
- **Built (2nd session, same week):** §5 domain model in `core/src/domain.rs` — `Side`, `OrderType`×`TimeInForce` (validity enforced at construction), `OrderAction`, scaled-integer `Price`/`Qty` with strict decimal parsing + exact `Display` round-trip, id newtypes, engine-timestamped `Order`. 18 unit tests covering every rejection path. All gates green (build/fmt/clippy `-D warnings`/18 tests).
- **Researched:** Cargo Book workspaces + `[workspace.lints]` (primary); `actions-rust-lang/setup-rust-toolchain@v1` + Swatinem/rust-cache (primary docs); corrode.dev CI tips (secondary). Two disagreements resolved and logged: commit `Cargo.lock` (yes, for reproducibility); `rust-toolchain.toml` pin (no for now — local cargo is Homebrew, revisit Phase 2).
- **Benchmarks:** none (Phase 0 excludes them; CI runs a release-profile smoke job only).
- **Decided:** see `decision-log.md` — workspace scaffold row (2026-09-05).
- **Missed / plateaus:** none. Note: first real CI run still unproven until the next `git push` — badge live but not yet earned.
- **Next week:** TODO §5 domain model — `Side`/`OrderType`/`OrderAction` enums, integer price/qty representation, newtypes, `Order` struct; every item with a test.

### Week of 2026-08-21 (setup week)

- **Built:** Repo scaffolded — AGENTS.md, docs/ (plan, architecture, stack, benchmarks, phases, guardrails, research notes on exchange-core/ABIDES/LEAN/IMC, record templates, paper/venue/internships). Skills installed in-repo.
- **Researched:** Verified the four reference projects (exchange-core 2,602★, ABIDES 560★, LEAN 21,287★, IMC Prosperity 18,803 teams). exchange-core benchmark methodology captured.
- **Decided:** see `decision-log.md`.
- **Next week:** Phase 0 prep — Rust crash course, order-book mechanics, minimal order book with unit tests.

---

## Template

### Week of YYYY-MM-DD

- **Built:** [what was built / changed this week]
- **Researched:** [what was studied]
- **Benchmarks:** [numbers + methodology pointer, if any]
- **Decided:** [link to decision-log rows]
- **Missed / plateaus:** [honest note if anything slipped]
- **Next week:** [one-line goal from the phase table]
