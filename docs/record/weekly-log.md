# Weekly log

> One entry per week, no missing weeks. Sunday afternoon: write the entry + plan next week. If a week is missed, backfill honestly ("missed, backfilled") — never fake an entry.

## 2026

### Week of 2026-09-05 (Phase 0 kickoff)

- **Built:** Rust workspace scaffold — virtual cargo workspace (resolver 3), `launchpad-core` crate (edition 2024, zero deps, smoke test), shared `[workspace.lints]` (unsafe forbid, missing_docs warn), rustfmt style_edition 2024, placeholder `bench/`/`sim/`/`venue/`, 4-job GitHub Actions CI (fmt / clippy `-D warnings` / test / release smoke), README getting-started + live CI badge. All four gates verified locally before commit.
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
