# Implementation Plan: §5 Risk/Accounting (minimal) — integer ledger + reserve-at-place

## Overview

The last Phase 0 code slice: per-user balances per currency (integers) and
place-time rejection of orders a user cannot fund. Design fork resolved by
primary-source research: **reserve-at-place** (exchange-core's model, evidenced
by `reservePrice` in their place-order API), not check-at-place — because
check-only lets two GTC bids over-commit the same free balance, and the order
book's own invariant ("no user ever owes the exchange") becomes a lie.

## Research basis

| Finding | Source | Consequence |
|---|---|---|
| exchange-core locks funds at place against a `reservePrice`; move needs no risk re-check | exchange-core README (primary) | Reserve at place; release on fill/cancel; adjust on move |
| Check-only fails: two GTC bids can both pass against one free balance | Derived (conservation argument) | Reserve model is a correctness requirement, not an optimization |
| Balances per (user, currency), integer amounts, deposit/withdraw commands, reject events first-class | exchange-core README | `CurrencyId` newtype; deposit/withdraw; `RiskError` |
| `cost_ticks = qty_lots × price_ticks / QTY_SCALE` needs rounding (global Phase 0 scales) | Derived from our constants | Ceil on charge (conservative); dust released on cancel/fill-completion; per-symbol scales stay Phase 3 |
| Margin = separate per-symbol mode | exchange-core README | Out of scope; direct-exchange semantics only |

## Architecture decisions

- **New module `core/src/risk.rs`** — risk is its own component
  (`docs/architecture.md`); the order book stays matching-only. No engine
  facade: wiring place→lock/fill→settle is Phase 1 engine work. Tests play
  the engine with scripted book+ledger sequences.
- **`free` + `locked` per (user, currency)**, both `u64` — an overdraft is
  *unrepresentable* (same philosophy as `Price(u64)` private-field).
- **Ceil-rounded quote costs** via one documented helper; buyer and seller
  settle the *same* computed amount per fill (no creation/destruction).
- **`CurrencyId(u64)` newtype** joins the domain newtypes.
- **Zero new dependencies** (hand-rolled error enum, per repo rule).

## Task list

- [ ] Task 1: `domain.rs` — `CurrencyId` newtype
- [ ] Task 2: `risk.rs` — `Balances`, `Ledger` (deposit/withdraw/commit/release/settle), `RiskError`, the ceil helper
- [ ] Task 3: Unit tests — deposit/withdraw paths, bid commit (insufficient → reject), ask commit, cancel release, move adjust (up needs free; down releases), settle transfer both directions
- [ ] Task 4: Integration test — scripted sequence: two users, deposits, crossing GTC orders, fills move quote+base correctly, cancel releases; played through real `OrderBook` + `Ledger`
- [ ] Task 5: Property test — conservation: total balance (all users, one currency) changes only by deposit/withdraw; per-fill buyer-paid == seller-received; no overflow panics under randomized ops
- [ ] Task 6: Four gates green; lib.rs module docs; decision-log rows (reserve model; ceil rounding); weekly log; TODO §5 ticks

## Acceptance criteria

- [ ] No code path can produce a negative or over-committed balance (u64 + checked math + rejections)
- [ ] Sum over users of (free+locked) is invariant under trading ops
- [ ] Buyer-paid == seller-received for every fill
- [ ] Cancel/move release exactly what's locked (no dust leaks, no double-release)
- [ ] All four CI gates green locally

## Risks

| Risk | Mitigation |
|---|---|
| Cross-currency rounding creates/destroys value | Same computed amount charged/credited per fill; ceil only on *locks* (dust returned); property test pins conservation |
| Scope creep into a full engine facade | Explicitly deferred to Phase 1; ledger API stays order-shaped but engine-agnostic |
