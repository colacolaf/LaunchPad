//! Launchpad CORE — the exchange hot path: order book, matching engine,
//! risk & accounting, event sourcing.
//!
//! Phase 0 status: the domain vocabulary ([`domain`]), the limit order book
//! ([`book`]), and the balance ledger ([`risk`]) are in; matching runs inline
//! in the book's `place` sweep. Phase 1 opens with the engine facade
//! ([`engine`]): the single owner that makes `place → lock → settle` atomic
//! over book + ledger.
//!
//! Project ground rules (see `docs/architecture.md`):
//! - `unsafe` is forbidden workspace-wide (`[workspace.lints]`).
//! - Every public item is documented (`missing_docs` is a CI gate).
//! - No floating point in any accounting or matching path.
//! - Determinism: no wall clock in the matching path.

#![deny(missing_docs)]

pub mod book;
pub mod domain;
pub mod engine;
pub mod risk;
