# Implementation Plan: Phase 1 Engine Facade (`core/src/engine.rs`)

## Overview

The Phase 1 opening task (decision log 2026-09-06): make `place → lock → settle`
atomic by giving the book + ledger pair a single owning component. The
integration test `core/tests/order_lifecycle.rs` currently *plays* the engine;
`Engine` makes it a real one. Correctness only — `docs/architecture.md`:
"Phase 1 is correctness; Phase 2 is speed."

## Research findings (what shaped the design)

1. **The settlement-rounding trap (new, found in this research pass).**
   The lock is `ceil(qty × P / QTY_SCALE)` — one ceil over the total. Phase 0's
   integration test settled each fill with the same ceil helper. That is only
   safe when every fill divides exactly: ceil is *subadditive*
   (`Σ ceil(xᵢ) ≥ ceil(Σ xᵢ)`), so dusty fills can settle MORE than the lock.
   Concrete failure: order of 3 lots @ 3 ticks locks `ceil(9/1e8) = 1` tick;
   three 1-lot fills settle `ceil(3/1e8) = 1` tick each → 3 settled > 1 locked
   → the ledger's `settle` panics. Phase 0 never hit it because the integration
   test used exact-divisible prices. **Decision: settle per fill with FLOOR**
   (`Σ floor ≤ floor(Σ) ≤ ceil(Σ) = lock`, mathematically safe), keep the ceil
   lock, release the dust with the lock remainder. Sellers take a sub-tick
   haircut vs. the exact price; per-symbol scales (Phase 3) make it exact.
2. **Market-bid funding (the open fork).** exchange-core's place carries a
   caller-supplied `reservePrice` — that is the reference answer, and it maps
   exactly onto our book: a market bid with reserve R is *semantically identical*
   to a Limit-IOC bid at R (sweep up to R, kill the remainder, never rest). So
   `Engine::place` requires `reserve: Option<Price>`; a market bid rewrites to
   `Limit { price: reserve } + Ioc` internally, which also constrains the sweep
   so fills can never settle above what was locked. Market asks need no reserve
   (the seller's lock is base lots; quote received is never owed).
3. **The engine needs no book API change.** Everything (remaining qty, live
   price, lock amount) is tracked in the engine's own per-order `LiveOrder` map,
   populated at place and updated per fill. The book stays pure matching; the
   engine owns money + lifecycle state.
4. **Saga/compensation ordering** (each step with a guaranteed-succeeding
   inverse):
   - place: commit → `book.place` → on book error, release (compensation);
     FOK-kill = release + `KilledFok` outcome (review flag (b) closed);
   - move (bids): funds check `free + old_lock ≥ new_cost` → `book.move_order`
     (WouldCross rejects *before* any money moves) → release old → commit new
     (cannot fail given the check). Review flag (a) closed.
   - cancel: book removes → release the whole per-order lock (dust included).

## Architecture

```
Engine { book: OrderBook, ledger: Ledger, quote/base: CurrencyId,
         live: HashMap<OrderId, LiveOrder> }
LiveOrder { user, side, price, remaining, currency, locked }   // Copy
```

- `EngineOutcome::{ Executed { fills, resting }, KilledFok }` — killed FOK is
  distinguishable from a zero-fill IOC.
- `EngineError::{ Book, Risk, MarketBidRequiresReserve, UnexpectedReserve,
  OrderTooLarge, InsufficientForMove }` — wraps the two layer errors plus the
  engine's own rejections.
- Settlement rule (one rule, both directions): **buyer pays
  `floor(fill_qty × price / QTY_SCALE)` quote from its lock; seller pays
  `fill_qty` base lots from its lock; payees receive free.** Self-trade
  (payer == payee) already correct in the ledger.
- Per-order lock invariant (property-tested): every live ask locks exactly its
  remaining lots; every live bid locks ≥ its exact remaining obligation.

## Tests

- **Unit:** full lifecycle through the engine; the dusty `3 lots @ 3 ticks`
  scenario as the rounding regression (would panic under per-fill ceil); FOK
  kill; unfundable order never reaches the book; duplicate-id compensation;
  cancel releases full lock incl. dust; move top-up sufficient/insufficient;
  move down releases; ask move touches no money; WouldCross leaves money alone;
  market bid requires reserve; market bid sweeps + releases leftover; self-trade
  conservation; partial-fill lock sufficiency.
- **Property (widened — review flag (c) closed):** randomized ops *through the
  engine* — GTC/IOC/FOK/market-with-reserve places, cancels, moves — asserting
  after every op: per-currency global conservation (Σ free+locked == deposits)
  and the per-live-order lock sufficiency invariant. Dusty ranges
  (price 1..=2000 ticks, qty 1..=10M lots) force the rounding paths constantly.
- Book-level invariants keep running unchanged (engine composes them).

## Acceptance criteria

1. All existing 64 tests still green (no regressions; integration test remains
   valid as the primitive-contract documentation).
2. New unit + property tests green under all four gates.
3. The three review-flagged Phase 1 traps each closed by code + test.
4. Decision log rows for the two new decisions (floor settlement, market
   reserve); weekly log; TODO Phase 1 section opened and first items ticked.
