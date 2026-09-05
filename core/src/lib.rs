//! Launchpad CORE — the exchange hot path: order book, matching engine,
//! risk & accounting, event sourcing.
//!
//! Phase 0 scaffold. This crate intentionally contains no domain code yet:
//! `docs/TODO.md` §5 (the minimal order book) is the next build session.
//! Everything below is a smoke placeholder proving build → test → CI works
//! end-to-end, and gets deleted when the real domain model lands.
//!
//! Project ground rules honored here (see `docs/architecture.md`):
//! - `unsafe` is forbidden workspace-wide (`[workspace.lints]`).
//! - Every public item is documented (`missing_docs` is a CI gate).
//! - No dependencies until a phase actually needs one.

/// Smoke placeholder: verifies the crate compiles, links, and runs in CI.
///
/// Returns the crate name so the test below and the future CLI demo (TODO §8)
/// have something honest to print. Delete with the rest of the scaffold once
/// `docs/TODO.md` §5 lands.
pub const fn crate_name() -> &'static str {
    "launchpad-core"
}

#[cfg(test)]
mod tests {
    use super::crate_name;

    /// Smoke test: the sole purpose of the Phase 0 scaffold is a green
    /// `cargo test --workspace` in CI. One assertion, one job.
    #[test]
    fn scaffold_builds_and_runs() {
        assert_eq!(crate_name(), "launchpad-core");
    }
}
