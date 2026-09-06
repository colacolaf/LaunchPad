//! Launchpad CORE — the exchange hot path: order book, matching engine,
//! risk & accounting, event sourcing.
//!
//! Phase 0 status: the domain vocabulary ([`domain`]), the limit order book
//! ([`book`]), and the balance ledger ([`risk`]) are in; matching runs inline
//! in the book's `place` sweep. The engine facade that makes book + ledger
//! operations atomic is the Phase 1 opening task (`docs/TODO.md` §5).
//!
//! Project ground rules (see `docs/architecture.md`):
//! - `unsafe` is forbidden workspace-wide (`[workspace.lints]`).
//! - Every public item is documented (`missing_docs` is a CI gate).
//! - No floating point in any accounting or matching path.
//! - Determinism: no wall clock in the matching path.

#![deny(missing_docs)]

pub mod book;
pub mod domain;
pub mod risk;
