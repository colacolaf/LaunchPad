# Implementation Plan: Phase 0 — Workspace scaffold + CI (TODO §1)

## Overview

Scaffold the Rust workspace for Launchpad's CORE layer: a virtual cargo
workspace with the `core` crate, placeholder dirs for `bench`/`sim`/`venue`,
a centralized lint policy, and a live GitHub Actions CI pipeline
(fmt / clippy / test / release smoke job). Correctness-first: no domain code,
no benchmarks, no optimization — this session only settles the layout so the
next session starts at the domain model (`docs/TODO.md` §5).

## Research findings (what makes this work)

Sources: Cargo Book §Workspaces (primary), `actions-rust-lang/setup-rust-toolchain`
README (primary), Swatinem/rust-cache README, corrode.dev "Tips for Faster Rust CI"
(secondary). Verified 2026-09-05.

1. **Virtual workspace** (no root package) fits Launchpad: CORE is Rust, SIM/VENUE
   are Python/TS — only `core/` is a cargo member today, more crates may come
   (e.g. `bench` in Phase 2). All members share one `Cargo.lock` + `target/`.
   A virtual manifest **must** set `resolver` explicitly; `resolver = "3"` is the
   edition-2024 resolver.
2. **Lint policy lives once**, in `[workspace.lints]` at the root; member crates
   opt in with `[lints] workspace = true` (Cargo ≥ 1.74). Phase 0 rule "no unsafe"
   becomes compiler-enforced: `unsafe_code = "forbid"`.
3. **CI stack**: `actions/checkout@v6` + `actions-rust-lang/setup-rust-toolchain@v1`
   (installs rustup toolchain, bundles Swatinem/rust-cache, provides cargo/clippy
   problem matchers, sets `RUSTFLAGS="-D warnings"` by default). Gates run as
   separate parallel jobs — independent signals, no shared cache contention.
4. **Smoke benchmark placeholder**: `cargo build --release` + `cargo test --release`
   verifies the profile the Phase 2 criterion harness will run under. Criterion
   itself is deferred to Phase 2 (per `docs/TODO.md` §10 — no premature setup).

### Points of disagreement (skeptic pass)

| Question | Old/default view | Current view | Our call |
|---|---|---|---|
| Commit `Cargo.lock` for lib crates? | Ignore it (pre-2023 guidance) | Commit it (Cargo recommends for all projects wanting pinned builds) | **Commit.** Determinism/reproducibility is a core project property; workspace will hold benches/binaries. `.gitignore` edited accordingly. |
| Pin toolchain via `rust-toolchain.toml`? | Pin for reproducibility | Pin binds CI only when local has no rustup (user's cargo is Homebrew) | **No pin in Phase 0.** CI tracks stable; revisit at Phase 2 when benchmark reproducibility makes the toolchain fingerprint material. Logged for the record. |

## Architecture decisions

- **Virtual workspace root** `Cargo.toml`: `resolver = "3"`, `members = ["core"]`,
  shared `[workspace.package]` metadata (version 0.1.0, edition 2024, MIT,
  rust-version 1.85 = first edition-2024 release).
- **Workspace lints**: `unsafe_code = "forbid"`, `missing_docs = "warn"`,
  `clippy::all = warn` — inherited by every member. CI runs with `-D warnings`,
  so warnings gate merges.
- **`rustfmt.toml`**: only `style_edition = "2024"` (pins formatting style
  regardless of toolchain). Everything else stays rustfmt defaults — TODO §1
  says commit config *only* for non-defaults.
- **CI**: 4 jobs — `fmt`, `clippy`, `test`, `smoke-bench` (release build + release
  tests). Least-privilege `permissions: contents: read`. Concurrency group cancels
  superseded runs.
- **`core/src/lib.rs`**: crate docs + one documented smoke function + one test —
  exists solely to prove build→test→CI end-to-end; deleted when the §5 domain
  model lands.

## Task list

### Task 1: Root workspace manifest (XS)
- [ ] `Cargo.toml` (virtual): resolver 3, members, workspace.package, workspace.lints
- Verification: `cargo metadata --no-deps` resolves; `cargo build --workspace` green

### Task 2: `core` crate skeleton (S)
- [ ] `core/Cargo.toml` (inherits workspace fields + lints)
- [ ] `core/src/lib.rs` (docs + smoke fn + test)
- Verification: `cargo test --workspace` runs 1 test green

### Task 3: Placeholder dirs (XS)
- [ ] `bench/README.md`, `sim/README.md`, `venue/README.md` — what lands here, when, and why it's empty now
- Verification: dirs exist, READMEs reference the owning phase docs

### Task 4: Formatting/lint config (XS)
- [ ] `rustfmt.toml` (style_edition 2024)
- Verification: `cargo fmt --all -- --check` clean

### Task 5: CI workflow (S)
- [ ] `.github/workflows/ci.yml`: fmt / clippy / test / smoke-bench jobs
- Verification: YAML valid; jobs mirror the four local gates in TODO §6

### Task 6: Repo hygiene (S)
- [ ] `.gitignore`: stop ignoring `Cargo.lock`
- [ ] README: CI badge live, getting-started commands, layout, status line
- [ ] `docs/TODO.md` §1: tick completed boxes with dated annotations
- [ ] `docs/record/decision-log.md`: append scaffold decision row
- Verification: no remaining "pending" CI badge; TODO §1 boxes match reality

### Checkpoint: all four gates green locally
- [ ] `cargo build --workspace`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] Self-review against code-review-and-quality axes before handing back

## Risks and mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Local toolchain is Homebrew (no rustup) — clippy/rustfmt components may be missing locally | Gates can't be verified locally | Check component presence first; CI installs its own toolchain regardless |
| `RUSTFLAGS=-D warnings` (CI default) + workspace `missing_docs = warn` | First CI run could fail on doc warnings | All public items documented in this scaffold; verified locally with clippy -D warnings |
| Badge URL wrong (remote is camelCase `LaunchPad`) | Fake-green badge — exactly what guardrails forbid | Use exact remote: `github.com/colacolaf/LaunchPad` (from PLAN.md §15) |
| Fake domain content sneaks in | Violates "explain every line" + scope rules | Scaffold contains only the smoke placeholder, explicitly marked for deletion |

## Open questions
- None blocking. Toolchain pinning deferred to Phase 2 by design.
