# Phase 0 walkthrough — the fluent explanation of your own code

> **How to use this.** This is a tutor-script over the code you now own:
> `core/src/domain.rs` (686 lines) and `core/src/book.rs` (1,143 lines). The
> rule from `AGENTS.md` §3.6 is the point: *you must be able to explain every
> line you ship.* So each lesson has four parts:
>
> 1. **Read** — open the anchored code first. Line numbers are approximate
>    (code drifts; function names don't) — search by name.
> 2. **Guided why** — worked reasoning, as your tutor would say it out loud.
>    Don't skip; this is the worked-example part of the method.
> 3. **Check yourself** — answer *out loud, from memory* before moving on.
>    If you can't, you just found the gap early, which is the whole game.
> 4. **Do** — a prediction exercise: write down what the code does *before*
>    running it, then verify. Predictions you get wrong are the highest-value
>    moments in this whole document.
>
> Then close everything and take `self-test.md` cold. The exit gate (TODO §9)
> is the 3-sentence price-time-priority exercise in there.

---

## Lesson 1 — Ownership & borrowing: the split borrow that runs the auction

**Read:** `book.rs` → `OrderBook::place` (~L440–490), the `let (own, opposite)`
match; `BookSide::sweep_against` (~L246); `OrderBook::side_ref` (~L565).

**Guided why.** The single most important borrow in the codebase is this:

```rust
let (own, opposite): (&mut BookSide, &mut BookSide) = match order.side {
    Side::Bid => (&mut self.bids, &mut self.asks),
    Side::Ask => (&mut self.asks, &mut self.bids),
};
```

Two `&mut` exist at once, which normally the borrow checker forbids — two
mutable references to the same data is the one thing Rust exists to prevent.
It compiles here because the two references point at **disjoint fields**
(`self.bids` vs `self.asks`). The compiler proves field-level disjointness,
so you get two independent mutable handles without any unsafe. This pattern
is called a *split borrow*.

Then `place` hands `opposite` to the sweep and also passes `&mut self.index` —
a third borrow of a *different* field — because the sweep must evict
fully-filled makers from the id index (Lesson 8 covers why that's load-bearing).
Notice `own` and `opposite` are used at different times: the sweep finishes
before `own.rest(...)` runs. Overlapping *lifetimes* is fine; overlapping
*use* is what the checker rejects.

Finally, note `side_ref(&self)` vs the inline match. The read-only accessor
can return `&BookSide` freely (shared borrow). The place path can't use it,
because it needs two `&mut` at once — that's why the match is written out
inline there. One accessor for reads, one specialized pattern for the
dual-mutable write path.

**Check yourself.**
- Why are two `&mut` in the tuple legal here but `(&mut self.bids, &mut self.bids)` illegal?
- Why does `sweep_against` need the index passed in, instead of reaching for it?

**Do.** Prediction: in `place`, swap the match arms' *targets* (bid returns
`(&mut self.asks, &mut self.bids)`), leave the rest unchanged. Without
running anything, list which tests break and what behavior you'd see.
Then run `cargo test` and grade your prediction.

---

## Lesson 2 — Move semantics & `Copy`: the maker that pops and comes back

**Read:** `book.rs` → `RestingOrder` (~L143, note `#[derive(Debug, Clone, Copy)]`);
the inner loop of `sweep_against` (~L265–303), `pop_front` → partial →
`push_front`; `Taker` (~L330).

**Guided why.** The matching loop does something that looks like it should
need `clone()`: it pops a maker *out* of the queue, mutates its `remaining`,
and pushes it back if partially filled. It works without cloning because
`RestingOrder` is `Copy` — 32-ish bytes of plain integers, so a move *is* a
copy, by design. Popping gives you ownership; pushing back donates it again.
If `RestingOrder` weren't `Copy` this exact code would still compile (you own
the popped value), but the derive is honest about the type's nature: it's a
 POD value, cheap to duplicate, carrying no heap.

Contrast with what is deliberately *not* Copy: `Order` (it lives on the
command path) and the `Fill` vec (heap allocation). The `Fill`s are built
once and handed to the caller in `PlaceOutcome` — a move, no copy. The rule
of thumb this codebase follows: Copy for small pure-value types the book
shuffles internally; move for anything that crosses an API boundary or owns
memory.

`Taker` exists for a reason worth being able to say: `sweep_against` needed
`taker_remaining`, `taker_id`, and `taker_user` as three parallel arguments,
which tripped clippy's `too_many_arguments`. Instead of silencing the lint,
the three fields were recognized as *one concept* — the incoming order
mid-execution — and bundled. The lint caught a modeling smell, not just style.

**Check yourself.**
- Why does `pop_front` → mutate → `push_front` compile without `.clone()`?
- What would you lose if `Fill` were `Copy`? (Hint: what does it contain?)

**Do.** Prediction: delete `Copy` from `RestingOrder`'s derive list and run
`cargo build`. Before running, write down which *specific* expressions you
expect to fail and why ownership (not Copy-ness) is or isn't enough at each.
Run it. Grade yourself. Restore the derive with `git checkout core/src/book.rs`.

---

## Lesson 3 — `String`/`&str`, parsing, and the no-float rule

**Read:** `domain.rs` → `parse_scaled_decimal` (~L110–193), all of it, line
by line; `Price::Display` (~L242) and `Qty::Display` (~L330);
`PRICE_SCALE`/`QTY_SCALE` consts (~L23–35).

**Guided why.** `parse_scaled_decimal(s: &str, scale: u64)` takes a borrowed
string slice — it never owns, never allocates. It walks *bytes* (`s.bytes()`,
`is_ascii_digit`), not chars, because a decimal string is ASCII by definition;
any non-ASCII byte fails the digit check and is rejected. That's the "treat
external input as untrusted" boundary for the whole exchange: every price and
quantity enters through this one strict parser. The exchange-core rule it
enforces: **no float ever touches a price or quantity** — because `0.1 + 0.2
!= 0.3` in binary floating point, and a matching engine that loses one ulp
creates phantom inventory.

The parse is a fold with checked arithmetic:
`int_val = int_val.checked_mul(10)?.checked_add(digit)?`. Overflow can't wrap
into a wrong-but-plausible number; it becomes `Err(Overflow)`. The fraction is
parsed as an integer then shifted *left* into scale position by multiplying by
`10^(missing places)` — so `"100.5"` against `PRICE_SCALE = 10_000` becomes
`1_005_000` ticks. Rejection rules to be able to enumerate: no sign, no
whitespace, no exponent, no second dot, integer part required (`.5` dies),
fraction required if a dot exists (`5.` dies), precision beyond the scale dies
(`TooMuchPrecision`), zero dies (`Zero` — a zero-priced order is a bug, not a
book state). Leading zeros are allowed: `007.5 == 7.5` is unambiguous, and a
rejection path nobody needs is code nobody should maintain.

`Display` is the exact inverse: divide by scale for the whole part, remainder
zero-padded to the scale's digit count (`{frac:0width$}`). Round-trip
guarantee: parse then display reproduces the canonical form — `100.5` →
`1_005_000` → `"100.5000"`. This is why the test asserts on the *string*, not
just the tick count.

**Check yourself.**
- Why is a float price dangerous even if you round? Name the failure mode.
- Why parse bytes instead of chars here?

**Do.** Predict `Ok`/`Err` (and which variant) for each call, then check with
a scratch test: `Price::from_quote_units_str` on `"0.5"`, `".5"`, `"5."`,
`"1.2.3"`, `"007.5"`, `"1.00001"`, `"0"`, `"-1"`; `Qty::from_base_units_str`
on `"0.00000001"` and `"0.000000001"` (why do these two differ in outcome?).

---

## Lesson 4 — Enums & pattern matching: modeling that forbids nonsense

**Read:** `domain.rs` → `Side` + `opposite()` (~L355); the `TimeInForce` doc
comment + enum (~L345–365); `OrderType` and `for_tif` (~L368+, search by name);
`BookError` + its `Display` (`book.rs` ~L60–120); `price_acceptable`
(`book.rs` ~L311).

**Guided why.** This codebase's enums are *designed*, not defaulted. The
original TODO sketch had five flat variants — `Limit, GTC, IOC, FOK, Market` —
and that model is wrong in a specific way: "GTC" *is* a limit order that
rests, so `Limit` and `GTC` overlap; and `Market × GTC` is a contradiction
(a market order cannot rest on a book — there is no price to rest at). The
shipped model factors the two *orthogonal* axes: **what** the order does
(`OrderType {Limit, Market}`) × **how long it lives**
(`TimeInForce {Gtc, Ioc, Fok}`), and `OrderType::for_tif` enforces the
validity matrix at construction — limit pairs with any TIF, market only with
IOC (market+FOK deferred to Phase 1, logged). Invalid states are
*unrepresentable*: there is no `Order` value in the entire system that means
"market GTC", and no code anywhere has to check for it. That's the payoff of
modeling discipline: a whole class of defensive `if` statements never gets
written.

Every `match` in the codebase is exhaustive — on `Side`, on `OrderType`, on
the error enums — so *adding a variant later becomes a compile error that
walks you to every site that must handle it*. That's not a style preference;
it's why this code can absorb Phase 1 changes without silent gaps.

`price_acceptable` is the whole matching policy in eight lines: `None` bound
means market (any price); a limit bid accepts maker prices **≤** its limit; a
limit ask accepts **≥** its limit. Equality crosses — and that's correct,
because the taker executes at the maker's price and never rests, so a bid at
1.00 meeting an ask at 1.00 trades at 1.00 and is gone; it never creates a
locked book.

**Check yourself.**
- Say back why Limit+GTC as *separate variants* was wrong, in two sentences.
- Why does a bid-at-the-ask trade rather than rest (what does the taker do
  with its remainder)?

**Do.** Prediction: a new `TimeInForce::Fak` (fill-and-kill, same as IOC —
it exists on real venues) is added to the enum. Without compiling, list every
place you expect the compiler to force a decision. Then add it, see how close
you were (note: `for_tif`'s match and every `matches!(order.tif, ...)` site),
and revert.

---

## Lesson 5 — `Option`, `Result`, and honest failure

**Read:** `book.rs` → the sweep loop's `let Some(...) = ... else break`
pattern (~L256–270); `cancel`'s `.ok_or(BookError::UnknownOrder)`
(~L520); the three `.expect("...")` calls (index/book sync, in `cancel`,
`move_order`, and the checked-sub sites ~L287–295); `domain.rs` `checked_add`/
`checked_sub` on `Price`/`Qty`.

**Guided why.** The codebase has a strict escalation ladder for failure:

1. **`Option`/`Result` returned** — when the caller can meaningfully react:
   `cancel` returns `Err(UnknownOrder)` because "cancel an already-filled
   order" is a *normal* race in a real venue (the cancel raced the fill).
   The glue between rungs is the **`?` operator**: `cancel` writes
   `self.index.remove(&id).ok_or(BookError::UnknownOrder { id })?` —
   `ok_or` converts the `Option` to a `Result`, and `?` is the
   *early-return-on-Err* shorthand that unwraps the `Ok` value or returns
   the error from `cancel` immediately. Every fallible step in `place` and
   `cancel` chains through `?`, which is why neither function contains a
   single `if let Err` — the error path is invisible until you need it.
2. **`checked_*` → `None`** — when arithmetic could overflow but the callers
   route `None` into an error anyway. Phantom inventory from a wrapped
   subtraction is the exact disaster `Qty::checked_sub` exists to prevent.
3. **`expect` with a stated invariant** — only where the condition is
   *structurally unreachable* given the rest of the design. Every `expect`
   message here states the invariant, not the symptom: `"index and book are
   in sync"`. The day that expect fires, it means the *design* broke, and the
   message tells you which invariant to investigate. This is the honest
   middle ground between panicking bare and wrapping unreachable states in
   `Result` the caller can't act on.

And the ladder's bottom rung: `unwrap` is allowed in `#[cfg(test)]` only
(shown by the test builders at the top of the test module) — in tests, a panic
*is* the failure signal you want. Shipped code has zero unwraps; that's also
lint-enforced territory (`clippy::unwrap_used` would be the gate, currently
covered by review discipline).

**Check yourself.**
- Why is `cancel` of a filled id `Err` rather than `Ok(0)`? (Think races.)
- Say back the three rungs of the ladder and one example of each from the code.

**Do.** Prediction: in `sweep_against`, the outer loop runs
`let Some(best) = self.best() else { break }`. Under what two distinct
conditions does that `break` trigger? Write both in domain language ("the
book side is …" / "the next level is …"), then trace one crossing order that
exercises each.

---

## Lesson 6 — Modules & visibility: making illegal states *invisible*

**Read:** `domain.rs` → `pub struct Price(u64)` (~L199) and `Qty(u64)`; the
private `ParseFailure` enum (~L76) vs public `DomainError` (~L47);
`lib.rs` (module list + doc comment); `book.rs` → which types are `pub` and
which are private (`RestingOrder`, `PriceLevel`, `BookSide`, `Taker`,
`Locator` are all private).

**Guided why.** `Price(u64)` has a *private* inner field. Nobody outside the
module can write `Price(0)` or `Price(u64::MAX)` or construct one from a
float — the only doors in are `from_ticks` (rejects zero) and
`from_quote_units_str` (the strict parser). This is the no-float rule enforced
by the type system rather than by convention: the invariant lives with the
type, so every future caller gets it for free. Same trick in `book.rs`:
`RestingOrder`, the locator index, and `Taker` are private because they are
*implementation* — the public surface is exactly the six operations a venue
needs (`place`, `cancel`, `move_order`, best-price accessors, counts). A
smaller public surface isn't just tidiness; it's every line of code that can
never exist to break an invariant.

`ParseFailure` vs `DomainError` shows the boundary drawn with intent: the
parser's internal failure vocabulary (including details like
`TooManyDecimals` needing the caller's scale context) stays private; the
public error carries context (`allowed: 4`) and is what callers match on.
The private enum exists *once* (review rule: no near-duplicate helpers), and
`map_failure` translates at the boundary.

**Check yourself.**
- What could a caller break if `Price`'s inner `u64` were `pub`?
- Why is `Taker` private when `Fill` is public?

**Do.** Prediction: write (don't commit) a snippet in a *new* file under
`core/src/` that tries `let p = Price(0);` and one that tries
`price.0 = 5;`. Predict the exact compiler errors you'll get, then run
`cargo build` and compare.

---

## Lesson 7 — Traits: the contracts the collections quietly require

**Read:** `domain.rs` → the derive lists on `Price`/`Qty` (`PartialEq, Eq,
PartialOrd, Ord, Hash`) and on `Side`/ids; `impl fmt::Display for Price`
(~L242) and for `BookError`/`DomainError` (`book.rs` ~L94, `domain.rs` ~L87);
`impl std::error::Error for ... {}`; `OrderAction` at the bottom of
`domain.rs`.

**Guided why.** Two derives on `Price` are load-bearing far beyond their
innocent look: **`Ord`** is what allows `BTreeMap<Price, PriceLevel>` to
exist — `BTreeMap` requires ordered keys, and best-price lookup
(`iter().rev()` for bids, `iter()` for asks) *is* the `Ord` impl doing the
work. **`Hash` + `Eq`** are what allow `HashMap<OrderId, Locator>` — a hash
map key must hash *and* compare equal consistently. Delete either derive and
the compiler names the exact trait bound that failed. That's the trait system
working for you: the collection documents its requirement, the type declares
its capability, and the check is mechanical.

`Display` is implemented manually, never derived, for one reason: the exact
round-trip (Lesson 3). A derived debug print or float formatting would break
the parse-display contract that the tests pin. And `impl std::error::Error
for DomainError {}` is an empty impl on purpose — the trait's methods all have
defaults; implementing it is how you opt *into* the `?`-conversion ecosystem,
letting `DomainError` ride `Box<dyn Error>` and compose with any other error
type later.

`OrderAction {Place, Move, Cancel}` rounds out the vocabulary — it's the
engine-layer dispatch table waiting to happen (the `const` assertion at the
bottom of book.rs's tests pins the type's shape). Phase 1's engine will match
on it one-to-one against the book's methods.

Two more trait lessons hide in `book.rs`, both worth being able to point at.
First, **generics vs `dyn Trait`**: `levels_in_match_order` returns
`Box<dyn Iterator<Item = (&Price, &PriceLevel)> + '_>` — a *trait object*.
The two branches (`iter().rev()` for bids, `iter()` for asks) produce
different concrete iterator types, and a single return type must name one
type; `Box<dyn Iterator>` erases the difference behind a vtable pointer at
the cost of one heap allocation. That trade is *correct here* — it's a query
path, not the hot path (the code comment says exactly this). A generic
`fn levels_in_match_order<I: Iterator...>` couldn't work: the *caller* didn't
choose the branch, the receiver's `best_is_highest` flag did. Know this
distinction: generics = compile-time dispatch, zero cost, one monomorphized
copy per concrete type; `dyn` = runtime dispatch, vtable indirection, one
code path.

Second, **iterators and the memory-layout story**. The aggregation methods
(`len`, `total_lots`) are pure iterator chains — `.values().flat_map(...)
.map(...).sum()` — and the zero-cost claim is that the compiler fuses these
into the same machine code as the hand-written loop. The memory-layout
corollary is visible in the data structure choices: `VecDeque` is a ring
buffer (amortized O(1) push/pop at *both* ends — what a FIFO queue needs,
where a `Vec` would pay O(n) to pop from the front), and every heap
indirection (`Box`, the `BTreeMap` nodes, the boxed iterator) is a potential
cache miss. Phase 2's arena redesign exists precisely to trade these
pointer-chasing allocations for contiguous memory — you can't understand
*why* that helps without first seeing where the indirections live today.

**Check yourself.**
- Which trait does `BTreeMap<Price, _>` require, and which does
  `HashMap<OrderId, _>` require?
- When is `Box<dyn Iterator>` the right choice over generics, and what does
  it cost? Name the site in `book.rs`.
- Why is the `std::error::Error` impl *empty*?

**Do.** Prediction: remove `Ord` from `Price`'s derives and `cargo build`.
Predict the error's *first line* before you run it. Then restore. (Yes, this
is the third "break it and predict the error" exercise — reading compiler
errors fluently is a Phase 0 skill in its own right.)

---

## Lesson 8 — The book itself: microstructure, end to end

**Read:** `book.rs` module docs (L1–40 — the design notes), `OrderBook`
struct (~L345), `place` in full, `move_order` (~L537), `price_acceptable`,
the `Fill`/`PlaceOutcome` types (~L43–80), and the four property tests at the
bottom of the test module.

**Guided why.** Everything assembles into one machine; walk the whole path:

**Structure.** One `BTreeMap<Price, PriceLevel>` per side; each level is a
`VecDeque` FIFO. The BTreeMap answers "what's the best price" in O(log P);
the deque *is* time priority — queue position = arrival order, no timestamps
needed anywhere in the book. That last sentence is worth dwelling on: the
data structure choice makes an entire category of clock questions vanish,
which matters because a wall clock in the matching path is the enemy of
determinism.

**Place.** Validate (symbol, duplicate id) → compute the bound (`Some(limit)`
or `None` for market) → FOK *pre-check* (`available_lots` measures before
anything mutates — all-or-nothing must never half-execute then discover a
shortfall) → sweep the opposite side best-first, one `Fill` per maker per
level, **always at the maker's price** (price improvement belongs to the
taker) → GTC remainder rests at the tail of its level; IOC/market remainders
die; FOK never partially executes.

**The bug the properties caught.** When the sweep fully fills a maker, it
pops from the level queue — but the id→locator `HashMap` still pointed at it.
A later `cancel` of that id would find the locator, miss in the level, and
panic on the sync expect. The fix: `sweep_against` takes the index and evicts
fully-filled makers. The regression test (`cancel_of_fully_filled_order_is_
unknown_not_panic`) pins it forever. Be able to tell this story — it's the
best answer you'll ever give to "tell me about a bug you caught."

**Move.** Repricing re-queues at the *tail* of the new level — queue position
is priority, so a move resets it (that's the exchange-core operation model;
amend-laundering priority is a real venue sin). And a move is the one
operation that could silently lock/cross the book (place sweeps itself; cancel
only removes), so it *rejects* with `WouldCross` rather than ever producing a
crossed state. The no-cross invariant is the spine: asserted after *every*
operation in the property replay.

**The conservation subtlety.** A crossing trade consumes quantity from *both*
sides but records *one* fill — so "placed = filled + canceled + resting"
only balances **per side**. The first property run failed until the ledger
booked every fill to both sides. A microstructure fact, discovered by test.

**Check yourself (the exit gate rehearsal).**
- Explain price-time priority in exactly 3 sentences, out loud, now, cold.
  (The self-test rubric grades this; the bar is TODO §9.)
- Why must FOK measure before mutating, in terms of what a caller could
  observe if it didn't?
- Why does the maker keep its queue slot on a *partial* fill but lose it on a
  full fill?

**Do.** Prediction: from an empty book — place ask 5 @ 1.00; place ask 4 @
1.01; place bid 6 @ 1.01 (GTC). Write down: every fill (maker id, price,
quantity) in order, the resting remainder, `best_bid`, `best_ask`, and
`total_resting_lots`. Then write it as a scratch test and run. This exact
sequence exercises: multi-level sweep, maker-price fills, and the partial
maker keeping its queue slot — while the *taker* fully fills, so nothing
rests and `best_bid` becomes `None`.

(Verified against the code during the docs audit — the tutor's first guess at
these numbers was wrong, which is rather the point of writing the test.
Extension question, worth answering before you run anything: what bid size
*would* leave a remainder resting, and is the book locked afterwards? Then
convince yourself that `place` can **never** rest into a lock — the sweep
consumes every level at or better than the limit first, so whatever rests
faces only strictly-worse prices — and that this is exactly why `place`
needs no `WouldCross` rejection while `move_order` does.)

---

## Coverage map — TODO §2/§3 → lessons

Read this as the audit table: every unchecked §2 item and every §3 item lands
in at least one lesson. (§3's "event sourcing basics" is read-only for now —
build is Phase 3 per §10; §4 reference study is separate reading, not here.)

| Checklist item | Lesson |
|---|---|
| §2 Ownership, borrowing, lifetimes | 1 |
| §2 Move semantics vs Copy; clone smell | 2 |
| §2 `&`/`&mut` aliasing rules | 1 |
| §2 `String` vs `&str`; `Vec` vs slices | 3, 2 (heap vs value types) |
| §2 Structs, enums, pattern matching | 4 |
| §2 `Option`/`Result`, `?`, no unwrap in shipped code | 5 |
| §2 Modules, crates, visibility | 6 |
| §2 Traits and generics; bounds; generics vs `dyn Trait` | 7 (`Box<dyn Iterator>` site) |
| §2 Iterators and the zero-cost claim | 7 (chains + memory-layout corollary; `cargo asm` session still separate) |
| §2 Memory layout: `Vec` vs array, cache-friendliness, why `Box`/heap hurts | 7 (`VecDeque` ring buffer, boxed iterator, Phase 2 arenas) |
| §2 Unsafe: know it, write none | 6 + the *enforcement*: `[workspace.lints]` `unsafe_code = "forbid"` in the root `Cargo.toml` — compiler-level, not convention |
| §2 Error handling: crate error enum early | 5, 7 |
| §2 Built-in `#[test]`/`#[cfg(test)]` | 5 (builders), 8 (properties) |
| §2 Property-based testing setup | 8 (the four invariants) |
| §2 Criterion — set up, don't optimize | deferred to Phase 2 per §10 (§1 already notes the deferral) |
| §2 Test organization: unit in-module, integration in `tests/` | all current tests are in-module `#[cfg(test)]` (`domain.rs`, `book.rs`); `tests/` arrives with cross-crate integration (Phase 1+) |
| §2 `cargo build/run/test/bench/doc/fmt/clippy/update` — know each | anchored in the CI workflow (`.github/workflows/ci.yml`: fmt/clippy/test) and the local four-gate checkpoint; `bench`/`doc` demoed when Phase 2 lands |
| §2 `std::sync` basics; why hot path avoids locks | — (single-threaded Phase 0; revisit Phase 2) |
| §2 Read `rust-best-practices` ch. 1–9 alongside | process item — the chapters were used while writing `domain.rs`/`book.rs`; re-read ch. 2 (ownership) + ch. 9 (testing) *after* this walkthrough |
| §3 Price-time priority | 8 (exit gate) |
| §3 Bid/ask, spread, crossed/locked | 4, 8 (spread = `best_ask − best_bid`; computable from the Lesson 8 Do exercise) |
| §3 Continuous double auction | 8 |
| §3 Limit/GTC/IOC/FOK/Market semantics | 4, 8 |
| §3 Order actions: place/move/cancel | 8 |
| §3 Maker vs taker (fee intuition) | 8 (maker-price rule) |
| §3 No floating point; representation | 3, 6 |
| §3 Event sourcing basics | pointer: Phase 3 build; read `docs/research/exchange-core.md` §journal |
| §3 exchange-core latency table shape | separate reading session (§4) |

Two honest gaps, deliberately: `std::sync` (nothing concurrent exists yet —
there is nothing real to explain, and faking it would violate the honesty
rule) and `cargo asm`/zero-cost-iteration assembly reading (worth a session
of its own; flagged for the weekend, not smuggled into a doc).

---

## After the walkthrough

1. Sleep on it (spacing is part of the method).
2. Take `self-test.md` closed-book, timed, out-loud answers.
3. Grade with `answers.md`; for every miss, re-read the cited code section —
   not the answer text, the *code*.
4. Day 2: re-take only the questions you missed. Day 7: re-take the full §9
   exit exercise.
5. Tick §2/§3 boxes in `docs/TODO.md` only for items you passed unaided —
   the record rewards honesty, per §7.
