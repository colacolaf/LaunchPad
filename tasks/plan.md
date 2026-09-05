# Implementation Plan: Phase 0 — §5 Domain model

## Overview

The first real domain code: sides, order types, time-in-force, scaled integer
prices/quantities, ids, and the `Order` value — each piece documented and
unit-tested. No order book yet (next task); this is the vocabulary the book
and engine will speak. Correctness-first, zero dependencies.

## Architecture decisions

- **Scaled integers, no floats (adopted from exchange-core):**
  `PRICE_SCALE = 10_000` ticks per quote unit (0.0001 precision),
  `QTY_SCALE = 100_000_000` lots per base unit (satoshi-style). Global
  constants in Phase 0; per-symbol scales are a Phase 3+ concern (logged).
- **`OrderType { Limit{price}, Market }` × `TimeInForce { Gtc, Ioc, Fok }`**
  instead of the TODO's flat 5-variant list: Limit and GTC would otherwise be
  redundant variants, and Market+GTC / Market+FOK are meaningless — validity
  is enforced at construction. **Deviation from TODO §5 wording, logged.**
- **Price/Qty are nonzero by construction** (`from_ticks`/`from_lots` reject 0),
  so order construction can't mint a zero-price limit order.
- **Decimal strings parse via integer arithmetic only** — no `f64` anywhere;
  strict parsing (no `+`, no leading `.`, no whitespace, no extra precision).
- **`timestamp` is a u64 engine-assigned monotonic sequence** — never wall
  clock (determinism rule from `docs/architecture.md`).
- **Hand-rolled `DomainError`** — thiserror arrives when the error surface
  actually grows (dependency decisions are logged one at a time).
- **Plain u64 id newtypes** (no `NonZeroU64`) — simplest correct thing now;
  the `Option<T>` niche win is a noted Phase 2 refinement.
- The scaffold smoke placeholder (`crate_name()`) is deleted per its own
  contract in the scaffold commit; domain tests are the new green.

## Task list

- [ ] Task 1: `core/src/domain.rs` — constants, ids, Price/Qty with parse+Display, Side, TimeInForce, OrderType (+validity), OrderAction, DomainError, Order
- [ ] Task 2: Unit tests per the rust-testing skill (~15 focused tests: parsing, rejection paths, validity combos, round-trips)
- [ ] Task 3: `lib.rs` rewrite — crate docs + `pub mod domain;`, smoke placeholder removed
- [ ] Task 4: Docs — TODO §5 domain-model ticks, decision-log rows (TIF factorization, scaled-int constants, hand-rolled error), weekly-log Built line
- [ ] Checkpoint: build / fmt / clippy -D warnings / test all green

## Risks

| Risk | Mitigation |
|------|------------|
| TODO deviation (5 variants → type×TIF) surprises the user | Flagged in summary + decision-log row; refactor is contained if vetoed |
| Parser edge cases (overflow, precision) | Strict rules documented; explicit tests for each rejection path; u128 accumulation with checked conversion |
| Scope creep into book/matching | Plan stops at the vocabulary; §5 book items stay untouched |
