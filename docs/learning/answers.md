# Phase 0 self-test — answer key

> Grade only after the full closed-book pass (see `self-test.md` rules). For
> every miss, re-read the **cited code** — not this file — then re-take the
> missed questions on day 2. Line numbers are approximate; search by name.

---

## A. Ownership & borrowing

**A1.** It's a **split borrow**: the two `&mut` point at *disjoint fields* of
`self` (`bids` vs `asks`), and the borrow checker proves field-level
disjointness. Two `&mut` to the *same* field would be aliasing — the one
thing Rust forbids. *Re-read: `OrderBook::place`, the `let (own, opposite)`
match (book.rs ~L458).*

**A2.** `side_ref(&self)` returns a single shared borrow — fine for read
paths. `place` needs **two mutable** borrows simultaneously, which the
helper's signature cannot express. The inline match is the specialized
dual-mutable form. *Re-read: `side_ref` (~L565) next to the `place` match.*

**A3.** Because the sweep **evicts fully-filled makers from the index** —
popping them from the level queue leaves a stale locator behind otherwise
(the exact bug the property suite caught; see A25). The index update and the
queue mutation must happen atomically inside the sweep. *Re-read:
`sweep_against`'s `index.remove(&maker.id)` (book.rs ~L299) and the
regression test.*

## B. Move semantics & Copy

**A4.** `RestingOrder` is `Copy` (plain integers, no heap), so the pop donates
ownership to the loop variable and the push-back donates it back — no borrow
of the queue is held across the mutation. Even without `Copy` it would
compile (ownership suffices); the derive declares the type's cheap-value
nature. *Re-read: the inner sweep loop (~L265–303).*

**A5.** Copy group = small, pure-value, no heap, shuffled internally by the
book. Non-Copy (`Order`, `Fill`) = command-path/heap-owning types that cross
API boundaries by move. `Fill` contains a `Vec`-born heap value inside
`PlaceOutcome` semantics; making it Copy would paper over that ownership
boundary. *Re-read: `RestingOrder` derive (~L143) vs `Fill`/`PlaceOutcome`
derives (~L43–80).*

**A6.** The three parameters were really **one concept** — the incoming order
mid-execution (`id`, `user`, `remaining`). The lint flagged a modeling smell;
the fix was naming the concept, not silencing the lint. *Re-read: `Taker`
struct + its doc comment (~L330).*

## C. Parsing & the no-float rule

**A7.** f64 is banned because binary floating point can't represent most
decimal fractions exactly (`0.1 + 0.2 != 0.3`), and a matching engine that
loses one ulp creates or destroys inventory — phantom fills. The replacement:
**scaled integers** — `Price` in ticks (`PRICE_SCALE = 10_000` per unit),
`Qty` in lots (`QTY_SCALE = 100_000_000` per unit), entered only through the
strict decimal parser. *Re-read: `domain.rs` module docs + scale consts
(~L23–35).*

**A8.** `1_005_000` ticks. Display prints `100.5000` — whole part
`100`, fraction `5000` zero-padded to 4 digits (`{frac:0width$}`). Round-trip
is exact because Display reads the raw remainder, no formatting floats. 
*Re-read: `parse_scaled_decimal` (~L110) + `Price::Display` (~L242).*

**A9.** Any three of: `".5"` / `"5."` (missing integer/fraction part →
`InvalidNumber`), `"1.2.3"` (second dot → `InvalidNumber`), `"-1"` (sign →
`InvalidNumber`), `" 5"` (whitespace → `InvalidNumber`), `"1.00001"`
(>4dp → `TooMuchPrecision { allowed: 4 }`), `"0"` (→ `Zero`). *Re-read: the
strictness rules doc + guard chain in `parse_scaled_decimal` (~L110–193).*

**A10.** A decimal string is ASCII by definition; `bytes()` avoids
per-char UTF-8 decoding and any non-ASCII byte simply fails
`is_ascii_digit` — so it's both faster and *more* strict (multi-byte
characters can't sneak through a char-level check that accidentally allowed
them). *Re-read: the byte loops in `parse_scaled_decimal` (~L136, ~L165).*

## D. Enums & modeling

**A11.** Problem 1: **redundancy** — "GTC" *is* a limit order that rests, so
`Limit` and `GTC` overlap. Problem 2: **nonsense states** — `Market × GTC`
is contradictory (a market order has no price to rest at). The fix factors
orthogonal axes: `OrderType {Limit, Market}` × `TimeInForce {Gtc, Ioc, Fok}`
with validity enforced in `for_tif`. *Re-read: `TimeInForce` doc comment
(domain.rs ~L345) + decision log 2026-09-05.*

**A12.** **Trade.** The taker executes at the maker's price (1.00) and never
rests, so no two *resting* orders ever sit at the same price on opposite
sides — which is what "locked" means. The book's invariant is about resting
state, and the taker leaves nothing resting. *Re-read: `price_acceptable`
(book.rs ~L311) + Lesson 8.*

**A13.** The **exhaustive `match`** — the compiler turns "add a variant" into
a walk through `for_tif`, every `matches!(...tif...)` site, and any dispatch.
That property is: invalid *extensions* are compile errors, the same way
invalid *states* are unrepresentable. *Re-read: `for_tif` and the test
builders.*

## E. Option/Result/expect

**A14.** Because in a real venue a cancel can **race a fill**: the order was
live when the request was created but fully filled by execution time.
`Err(UnknownOrder)` tells the truth to the caller; `Ok(0)` would invent a
"cancel of nothing succeeded" event. Distinguishing these matters for the
event journal and for client behavior. *Re-read: `cancel` (~L520) +
`cancel_of_fully_filled_order_is_unknown_not_panic`.*

**A15.** Rung 1 — **return `Result`/`Option`** when the caller can react
(`cancel` → `UnknownOrder`). Rung 2 — **`checked_*` arithmetic → `None`**
when overflow is possible and must never wrap
(`Qty::checked_sub` in the sweep). Rung 3 — **`expect` with a stated
invariant** only for structurally unreachable states
(`"index and book are in sync"`). `unwrap` exists only in tests. *Re-read:
the sweep's checked-sub expects (~L287–295) and `cancel`.*

**A16.** When it fires, the message names **which design invariant broke**,
not just that something went wrong — it converts a mystery panic into a
pointed investigation of the synchronization discipline between index and
book. *Re-read: the three expect messages in `cancel`/`move_order`/sweep.*

## F. Visibility & type boundaries

**A17.** (1) Anyone could construct `Price(0)` — destroying the non-zero
invariant that `from_ticks` guards. (2) Anyone could construct a price from
a float conversion or arbitrary huge value — destroying the no-float and
overflow invariants. The private field funnels every construction through
the validating doors. *Re-read: `pub struct Price(u64)` (~L199) and its two
constructors.*

**A18.** The criterion: **is it part of the venue's operation surface, or
implementation?** `Fill` is an output the venue reports (public);
`Taker` is scratch state internal to one sweep (private). Same split as
`RestingOrder`/`Locator` (private) vs `PlaceOutcome` (public). *Re-read:
lib.rs re-exports and the pub-ness of each type.*

## G. Traits

**A19.** `BTreeMap<Price, _>` requires **`Ord`** on the key (best-price
lookup *is* the `Ord` ordering — `iter().rev()` for bids). `HashMap<OrderId,
_>` requires **`Hash` + `Eq`**. *Re-read: `Price`'s derive list
(domain.rs ~L198) + Lesson 7.*

**A20.** Because the Display must guarantee the **exact decimal round-trip**
(`1_005_000` ticks → `"100.5000"`) — raw remainder, zero-padded, no float
formatting. A derived impl would print the tuple form and break the
parse↔display contract the tests pin. *Re-read: `Price::Display` (~L242).*

**A21.** It opts the error into the **`?`-conversion ecosystem**: every
`From`-based `?` conversion, `Box<dyn Error>` composition, and downstream
error handling recognizes it as a standard error. The trait's methods have
defaults; the empty impl is an opt-in. *Re-read: `impl std::error::Error for
DomainError {}` (~L95).*

## H. The book end to end

**A22.** (1) Validate: symbol match, then duplicate-id via the index. 
(2) Compute the **bound**: `Some(limit price)` for limits, `None` for market. 
(3) FOK **pre-check** via `available_lots` before any mutation. 
(4) **Sweep** the opposite side best-first, emitting fills at maker prices. 
(5) **Rest or die**: GTC remainder → tail of own level + index insert; 
IOC/market/FOK remainder dies. *Re-read: `place` top to bottom (~L440–490).*

**A23.** All-or-nothing must be decided **before** the first fill — otherwise
a partial execution could hit a shortfall mid-sweep and the caller would
observe fills that then need "un-filling", which the design has no operation
for. Measure first = the kill path leaves the book untouched. *Re-read: the
FOK block in `place` (~L437) + `available_lots`.*

**A24.** **Keeps.** Queue position *is* time priority; a partially-filled
maker arrived earlier and hasn't finished trading, so it stays at the front
and keeps trading first. On a full fill there is no order left — and it must
leave the index too (A25). *Re-read: the `push_front` branch in the sweep
(~L294) vs `index.remove` (~L299).*

**A25.** **What:** fully-filled makers were popped from their level queue but
left in the id→locator index; a later `cancel` of that id found the locator,
missed in the level, and hit the "index and book are in sync" expect. 
**Found:** the randomized property suite — minimal counterexample "place bid
1@21, place ask 1@1" — no unit test had ever cancelled a filled id. 
**Fix:** `sweep_against` takes `&mut self.index` and evicts fully-filled
makers; regression test pins it. *Re-read:
`cancel_of_fully_filled_order_is_unknown_not_panic`.*

**A26.** Tail re-queue is what "resets time priority" means when queue
position *is* priority — exchange-core's model, and anti-amend-laundering by
construction. Move needs `WouldCross` because place **sweeps** (it consumes
the crossing instead of resting into it) and cancel **only removes** — move is
the sole operation that would otherwise *rest* an order into a locked/crossed
book silently. *Re-read: `move_order` (~L537) + module docs L1–40.*

**A27.** Every fill is a match between **one bid and one ask** — 3 lots
consumed from the bid side *and* 3 from the ask side, recorded as one fill.
Per-side ledgers are the only books that balance; a single pooled ledger
double-counts. This was the second thing the first property run caught. 
*Re-read: the conservation property + the per-side ledger comment in the
test module.*

## I. Exit gate

**A28.** Model answer (any phrasing with these three elements):
"Matching always consumes the **best price** on the opposite side first —
highest bid, lowest ask — sweeping levels in that order. At **equal price**,
earlier arrival trades first; in this book, arrival order *is* the level's
FIFO queue position, so no timestamps are needed. Fills execute at the
**maker's price**, the taker's remainder rests or dies by policy, and no
operation can leave the book locked or crossed."

The rubric grades the three legs (price / time / correctness), not the words.
If you can produce all three legs cold, that's the §9 box.

---

*Every answer above was verified against the code as of commit `33e0fac`
(order book). If the code changed since, trust the code — and note the drift
in the weekly log.*
