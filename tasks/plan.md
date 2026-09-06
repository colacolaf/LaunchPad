# Implementation Plan: Phase 0 Order Book (place / cancel / move + best-price accessors)

## Overview

The second Phase 0 deliverable: a minimal, correctness-first limit order book
for `launchpad-core`, on top of the existing domain model. It supports the
exchange-core operation model (place / move / cancel), the three time-in-force
policies, market orders, and emits fill events. No optimization, no
concurrency, no journaling — those are Phase 2/3 per TODO §10.

## Architecture Decisions

- **`BTreeMap<Price, PriceLevel>` per side + `VecDeque<RestingOrder>` FIFO per
  level.** `BTreeMap` gives O(log P) sorted access to the best price with zero
  unsafe and zero dependencies; `VecDeque` gives O(1) push-back/pop-front for
  time priority. This is the standard-correctness design (RustQuant walkthrough,
  limitbook crate, multiple references agree). Faster structures (arenas, flat
  arrays, lock-free) are Phase 2 material — TODO §10 forbids optimizing now.
- **`HashMap<OrderId, Locator>` index.** O(1) cancel/move requires finding an
  order's level without scanning. Locator = (side, price) plus a FIFO position
  (monotonic per-book counter). Queue members keep an `enter_seq` so a moved
  order's slot is distinguishable; cancel marks removed entries as tombstones
  and pops them lazily from the queue front (O(1), no interior removal).
- **Matching at the resting price, not the taker price.** Price improvement
  belongs to the taker; every fill records the maker's limit. Exchange rule,
  encoded as a test.
- **Execution policies evaluated up front where possible:** FOK pre-checks
  available liquidity against the limit (all-or-nothing before any fill);
  IOC/market fill greedily then drop the remainder; GTC rests the remainder.
  Market orders never rest (domain already enforces Market × IOC only).
- **API returns `PlaceOutcome { fills, resting_qty }`** rather than mutating
  through callbacks — keeps the engine pure/deterministic and makes fill
  assertions direct. (Event-sourced journaling wraps this in Phase 3.)
- **`move` = remove + re-add at the tail of the new price level.** exchange-core
  treats move as the priority op; the TODO requires that move *resets time
  priority*. Keeping remaining qty and identity; only price (and thus level
  and priority) changes.

## Task List

### Phase 1: Book structure
- [ ] Task 1: `book.rs` — `RestingOrder`, `PriceLevel`, `BookSide` (bids: best
      = highest, asks: best = lowest), `OrderBook::new`, empty-book accessors.
- [ ] Task 2: `place` for non-crossing GTC limit orders — rests, updates best.
      Unit tests: rest, best-bid/ask update, level ordering.

### Phase 2: Matching
- [ ] Task 3: Limit crossing on `place` — fill at maker price, partial-fill
      remainder rests, full fill leaves no trace. Unit tests per TODO §6 rows 1–2.
- [ ] Task 4: Execution policies — IOC (no rest), FOK (pre-check, all-or-nothing),
      Market (sweeps levels at each level's price). Unit tests: §6 rows 3–5.
- [ ] Task 5: `cancel` + `move` with time-priority reset; tombstone handling.
      Unit tests: §6 rows 6–7. Error cases: cancel/move unknown id.

### Phase 3: Properties
- [ ] Task 6: proptest dev-dependency (decision-log row) + invariant property:
      **no crossed book** after any op sequence; plus **conservation** (fills =
      in − resting − cancelled) on the same generator.
- [ ] Task 7: Checkpoint — all four gates green (build, fmt, clippy -D warnings,
      test).

### Phase 4: Close-out
- [ ] Task 8: code-review-and-quality pass; tick TODO §5 (order book) + §6
      (unit rows covered); decision-log rows (book structure, proptest);
      weekly-log line; commit.

## Checkpoints
- After Task 2: resting-only book behaves, best accessors correct.
- After Task 5: full operation model works (place/move/cancel + all TIFs).
- After Task 7: CI-equivalent green locally.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Borrow-checker fight matching against `self.asks` while mutating `self.bids` | Med | Match through the two side handles via `match side` and split borrows with local `let (maker_side, taker_side)` pattern; keep matching inside `BookSide` methods where possible |
| Tombstones leaking memory / stale locators | Med | Pop tombstones eagerly at queue front during matching; assert locator queue not empty when level exists |
| FOK semantics ambiguity (budget vs quantity) | Low | exchange-core's FOK-B is budget-based; TODO says "fills entirely or nothing" — implement quantity-FOK per TODO, note the deviation in the module docs |
| proptest adds a dependency | Low | dev-dependency only; logged in decision log per repo rule |

## Open Questions
- None blocking — the TIF factorization from the domain model session already
  settled Market×FOK as deferred.
