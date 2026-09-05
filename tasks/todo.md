# TODO — Phase 0 Step 1: workspace scaffold + CI

- [x] Task 1: Root virtual workspace `Cargo.toml` (resolver 3, members, shared lints)
- [x] Task 2: `core` crate skeleton (`Cargo.toml` + `lib.rs` + smoke test)
- [x] Task 3: `bench/` `sim/` `venue/` placeholder READMEs
- [x] Task 4: `rustfmt.toml` (style_edition 2024)
- [x] Task 5: `.github/workflows/ci.yml` (fmt / clippy / test / smoke-bench)
- [x] Task 6: repo hygiene — `.gitignore` (commit Cargo.lock), README badge + getting started, TODO §1 ticks, decision-log row

## Checkpoint (all must be green before done)
- [x] `cargo build --workspace` — green (dev profile, 2.7s)
- [x] `cargo fmt --all -- --check` — clean
- [x] `cargo clippy --workspace --all-targets -- -D warnings` — clean
- [x] `cargo test --workspace` — 1 passed, 0 failed
- [x] Code-review pass done (five axes: approve); no fake domain code; decisions logged in `docs/record/decision-log.md`

## Verification story (2026-09-05)
- All four CI gates executed locally and passed before handoff.
- `.github/workflows/ci.yml` validated by YAML parse: jobs = fmt, clippy, test, smoke-bench.
- First real CI run happens on push — the badge is live but unproven until then (honest status).
- Note: pre-existing uncommitted changes in AGENTS.md / docs/PLAN.md / docs/phases.md / docs/stack.md /
  docs/research/rust-vs-java.md / docs/record/decision-log.md predate this scaffold session — review separately when committing.
