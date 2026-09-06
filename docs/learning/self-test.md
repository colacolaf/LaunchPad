# Phase 0 self-test — closed book, out loud, timed

> **Rules — these are the method, not bureaucracy.**
> 1. **Closed book.** No code open, no walkthrough open, no answers file. If
>    you catch yourself reaching for the editor, that's a miss — note it and
>    move on. (Desirable difficulty is the mechanism; defanging it defangs
>    the learning.)
> 2. **Out loud.** Speaking forces complete sentences, and complete sentences
>    expose half-understandings that nodding doesn't.
> 3. **Timed.** ~90 seconds per question; the §9 exit exercise gets 3 minutes.
>    Fluency means unscripted, not eventually.
> 4. **Write your answers down** before grading. Retroactive "yeah I knew
>    that" is the failure mode the written record exists to prevent.
> 5. Grade with `answers.md` only after the full pass. For every miss: re-read
>    the **cited code section**, not the answer text.
>
> **Scoring:** each question 1 point. 25–30: fluent, tick the §2/§3 boxes for
> what you passed. 18–24: solid, re-visit missed lessons, re-take missed
> questions tomorrow (spacing). Below 18: re-run the walkthrough lessons for
> the weak areas before re-testing.

---

## A. Ownership & borrowing (Lesson 1)

**Q1.** In `OrderBook::place`, the tuple `(&mut self.bids, &mut self.asks)`
holds two mutable references at once. Why is this legal?

**Q2.** Why does the same function write the side-match *inline* instead of
reusing the `side_ref` helper like the read paths do?

**Q3.** The sweep receives `&mut self.index` alongside the `opposite` side
borrow. Why does the sweep need write access to the index at all?

## B. Move semantics & Copy (Lesson 2)

**Q4.** The matching loop pops a maker from its level queue, mutates
`remaining`, and pushes it back — with no `.clone()`. Why does that compile?

**Q5.** `RestingOrder` derives `Copy`; `Order` and `Fill` do not. What
distinguishes the types in each group, and why does the split make sense?

**Q6.** `Taker` was introduced to satisfy clippy's `too_many_arguments`. What
was the *conceptual* (not lint) justification for bundling id/user/remaining?

## C. Parsing & the no-float rule (Lesson 3)

**Q7.** In one sentence each: why is `f64` banned from prices/quantities, and
what replaces it?

**Q8.** `"100.5"` parses against `PRICE_SCALE = 10_000`. State the exact tick
value and the exact `Display` output.

**Q9.** Name three decimal-string inputs the parser rejects that a casual
implementation would accept, and the error variants.

**Q10.** Why does the parser walk `bytes()` rather than `chars()`, and why is
that safe/correct here?

## D. Enums & modeling (Lesson 4)

**Q11.** The original §5 sketch was `OrderType {Limit, GTC, IOC, FOK, Market}`.
Name the two distinct modeling problems with that, in one sentence each.

**Q12.** A limit bid at 1.00 meets a resting ask at 1.00. Trade or rest — and
why is equality *not* a lock?

**Q13.** Adding a new `TimeInForce` variant: what walks you to every site that
must change, and what property of the code does that?

## E. `Option`/`Result`/expect (Lesson 5)

**Q14.** `cancel` of an already-filled id returns `Err(UnknownOrder)` rather
than `Ok(0)`. What real-world scenario does the `Err` handle honestly that
`Ok(0)` would paper over — and why does the difference matter downstream?

**Q15.** State the three-rung failure ladder used in the codebase, with one
real example of each rung from `book.rs`/`domain.rs`.

**Q16.** Every `expect` message states an invariant ("index and book are in
sync"). What is the discipline's purpose on the day the expect actually fires?

## F. Visibility & type boundaries (Lesson 6)

**Q17.** `Price`'s inner `u64` is private. Name two things a caller could
break if it were `pub`.

**Q18.** Why is `Taker` private while `Fill` is public? State the criterion,
not just the fact.

## G. Traits (Lesson 7)

**Q19.** Which trait derive makes `BTreeMap<Price, PriceLevel>` possible, and
which pair makes `HashMap<OrderId, Locator>` possible?

**Q20.** Why is `Display` hand-implemented rather than derived for `Price`?

**Q21.** What does `impl std::error::Error for DomainError {}` (an empty
impl) buy, mechanically?

## H. The book end to end (Lesson 8)

**Q22.** Walk a place through `place()`: name, in order, the five stages from
validation to resting, including where the bound comes from.

**Q23.** Why does FOK *measure* (`available_lots`) before any mutation, and
what would a caller observe if it didn't?

**Q24.** A maker is partially filled. Does it keep its queue slot? Why — in
terms of the priority rule?

**Q25.** Tell the index-leak bug story: what went wrong, how it was found,
and what the fix was. (Two or three sentences; this is the interview answer.)

**Q26.** Why does `move_order` re-queue at the *tail*, and why is move the
only operation that needs a `WouldCross` rejection?

**Q27.** A crossing trade consumed 3 lots. Why does the conservation ledger
book 3 on *both* sides?

## I. The exit gate (TODO §9) — do this last, 3 minutes

**Q28.** **Explain price-time priority in exactly 3 sentences.** No more, no
fewer. Cold, unscripted, out loud. Record yourself if you want proof for the
weekly log.

**Rubric** — all three elements, any order:
- **Price** leg: matching consumes the best price on the opposite side first
  (highest bid / lowest ask), sweeping levels in that order.
- **Time** leg: at equal price, earlier arrival wins — and in this codebase,
  arrival order *is* queue position in the level's FIFO (no timestamps).
- **Correctness** leg: fills execute at the *maker's* price; the taker's
  remainder rests (GTC) or dies (IOC/market); the book can never end locked
  or crossed.
3/3 elements: pass. 2/3: re-take in a day. Anything vague or memorized-sounding
without understanding: back to Lesson 8 first.

---

## Spacing schedule (built into the method)

- **Day 0:** walkthrough, then this test, closed book. Log the score in the
  weekly log (honesty rule).
- **Day 2:** re-take *only* the questions you missed, cold.
- **Day 7:** re-take Q28 plus any question family where you missed ≥1.
- Only after the day-7 pass do the §2/§3 checkboxes get ticked — and only
  for the items actually passed unaided.
