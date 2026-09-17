# Session 19 — 2026-09-17 — Phase 3 journal + replay (the Done-when slice)

## The plan (decisions written BEFORE code — the contract the tests hold me to)

**Decision A — command journaling, not event journaling.** The journal records *commands* (inputs) + their acceptance, not state-mutation events. A closed set maps 1:1 onto the existing public surface: `Deposit`, `Withdraw`, `SetPositionLimit`, `ClearPositionLimit`, `Place { order, reserve }`, `Cancel`, `Reduce`, `Move` — plus a header carrying `symbol/quote/base/FeeSchedule` (row 52's "schedule journaled for replay"). Event journaling would mean a second fill-application engine that can drift from the first; command replay IS the engine — fills and counterparty state are *derived* by replay and proven identical by digest, not stored. Exchange-core's model (orders + operations replayed through the same engine).

**Decision B — in-memory this slice; disk I/O + snapshots are the next increment, recorded as an explicit deferral.** The semantic core of "replay produces identical state" is record → fold → deep-digest equality, testable now. The byte format is a pure addition that changes nothing about identity. Snapshots are a recovery *performance* optimization (they matter when journals are long — a disk-scale concern); snapshot-less replay is the STRONGEST identity proof. Phase 3 cannot close until disk lands — the deferral is honest and recorded, not silent.

**Decision C — rejected commands are journaled too.** Every entry is `{ command, accepted }`; replay must reproduce the same accept/reject per entry. A mismatch is `ReplayError::Divergence { index, recorded, replayed }` — divergence is a bug, returned loudly, never papered over.

**Decision D — the deep digest (the Phase 1 carry-forward lands here).** `deep_digest(engine)` folds: header config, ALL ledger accounts (free/locked, sorted by (user, currency)), fee sink, position limits (needs a listing accessor), live orders (sorted by id: user/side/price/remaining/locked), and **book depth both sides in match order with per-level queue order** — price-time priority becomes digest-observable, which `resting_lots` aggregates cannot see. Needs one new public accessor: `OrderBook::depth(side) -> Vec<(Price, Vec<(OrderId, UserId, u64-lots)>)>`.

**Test list:** round-trip empty (header-only) journal; hand scenario (deposits → resting GTC pair → reduce → move → cancel) digest-equal; crossing scenario with fills + fees digest-equal; rejected-command recording (over-limit place) replay-agrees; withdrawal replay-agrees; hand-crafted wrong `accepted` flag → `ReplayError::Divergence` (not silent); `depth` accessor unit tests (two levels, queue order, bid desc/ask asc); digest bites on queue-order swap (proves the fold isn't aggregate-blind); **proptest Done-when: random biased command stream → record → replay → deep digest equality**.

## What was done (row 60 records the full story)

All eight planned tests landed (hand scenario became one combined stream; withdrawal-replay folded into the scripted rejection test). Plus the unplanned-but-necessary: the scripted rejection test exposed that my first-draft `record` contract was incoherent — trading rejections recorded as outcomes, money rejections escaping as `Err` (unrecorded state!). Fixed at the root: **`record` is infallible by protocol** — every command's outcome is deterministic and journaled. Decision rows 59 (design) + 60 (implementation) capture it.

## Proof

- **Done-when proven:** `replay_of_random_commands_produces_identical_state` — up to 120 biased random commands, record → replay → deep digest byte-equality (config + accounts + fee sink + caps + live orders + book depth with queue order). Divergence = loud error.
- Engine untouched: canonical 2k digest `0xea9d71ec0f18e3a3` byte-identical; 131 + 4 + 3 tests green; fmt/clippy `-D warnings` clean.
- Size discipline held: journal.rs 455 + journal/tests.rs 392 — split convention from day one.

## The gates caught me again (recorded honestly)

First draft failed 4 of 8 tests: the scenarios never funded **BTC** (asks lock base — every ask died at risk), and my sanity assert in the hand scenario was wrong (both crossings fully fill → book ends *empty*). Two accessor pattern bugs + a `fee_sink`/`fees_collected` name confusion died at compile/test. All pre-commit, all in row 60.

## What remains for Phase 3 (explicit, recorded)

Disk byte format + snapshots (row 59's deferral). Phase 3 does **not** close until disk lands — the deferral is in the TODO, not silent.

---

# Session 18 — 2026-09-17 — review findings executed (necessary changes)

## What was done

The review verdict had two optional findings. Necessary-change triage: finding 1 (size) was actionable immediately — its standing instruction was "decompose at the journal slice's seam before piling on," and the journal slice is next. Finding 2 (would-lock rule duplication) is **not** actionable: the trigger is a third money-in-motion path and only two exist — extracting now would violate the third-use rule the finding itself cited.

**Finding 1 executed — the mechanical split:**
- `core/src/engine/tests.rs` (1,497 lines) and `core/src/risk/tests.rs` (643) — test source moved verbatim (one dedent), no visibility changes, no content edits.
- `engine.rs` 2,216 → **719**; `risk.rs` 1,255 → **612** — all four files under the ~1,000-line inspection signal.
- Both impl files end at `#[cfg(test)] mod tests;` — same module path, zero API change.

## Proof of behavior-invisibility

- fmt + clippy `-D warnings` clean; **123 + 4 + 3 tests identical** (same names, same counts).
- 2k-op latency digest **byte-identical pre- vs post-split**: `0xd736d89a4102f8b4` measured against HEAD (stash → run → pop) in the same session — the honest comparison, not a stale recorded number.

## Decision

Row 58 records the split AND the finding-2 ruling (dormant, trigger written). The journal slice now starts from files that fit.

---

# Session 17 — 2026-09-17 — Phase 3 mid-phase review gate

## Outcome

**APPROVE — no Critical, no Required.** Scope: the full Phase 3 diff `1e2bcb2..9514560` (fees, position limits, reduceOrder, fee'd bench mode + records), tests read first per the skill, every code file's final state read end-to-end.

## Five axes

- **Correctness — clean.** Commit stays all-or-nothing (checked sub before add, pre-checked would-lock arithmetic with the named invariant). Fee legs dispatch on `taker_side` (self-trades correct by construction) and key on the *received* asset; each fee ≤ its own just-credited amount because `apply_fill` collects only after both settles. Reduce clamps at the book's invariant site; lock math rests on floor subadditivity over any split. Edge matrix in tests: cap=0 freeze, cap-below-lock, at-cap pass, pre-existing locks counted, per-account/currency independence, dusty-lock split releases, `u64::MAX` fee, the 10_000 bps boundary, clamped reduce, level teardown on full reduction.
- **Readability — clean.** Every non-obvious rule carries its why at the site (why caps key on locked; why the move pre-check duplicates commit's rule; why the dust convention is settlement's convention). No dead code from the mid-session bugs — the settle clobber was repaired byte-identical and the first-draft dead branch removed.
- **Architecture — clean.** Risk rules live in the ledger, money orchestration in the engine, the book stays money-blind. Bench fee config reads env at engine construction in support — one wiring point, all three harnesses inherit; malformed specs panic (fail-fast, never a silent fallback). Public surface unchanged in shape: modules re-exported by path, `missing_docs` deny-gated.
- **Security — clean.** No new dependencies; `unsafe` forbid untouched; env config validated + loud-panicking; no secrets, no untrusted input beyond the typed command surface.
- **Performance — clean.** The limit gate and fee legs sit *inside* the measured canonical run: 1M digest `0xc3bea4a3b66dd253`, p50 250 ns unchanged; the 100%-fee upper bound is disclosed as such; fee'd numbers never compared to the canonical baseline.

## Findings

- **Optional (deferred):** engine.rs (2,216 lines) and risk.rs (1,255) are past the ~1,000-line size-inspection signal — decompose at the journal slice's natural seam (test module and/or operation groups) *before* piling journal code on. Recorded in TODO as a standing instruction.
- **Optional (deferred):** the move-path cap pre-check duplicates commit's rule — justified by the never-fail-mid-way guarantee for now; a **third** money-in-motion path must extract the shared would-lock rule instead of copying it again.
- **Carry-forward (still open, now load-bearing):** replay digest folding book depth — lands with the journal slice.

## Verification

- fmt clean, clippy 0 errors, 123 lib + 4 + 3 integration tests green (as of slice 3's gates; CI re-runs on push).
- Review verdict + findings recorded in decision row 57 and docs/TODO.md (gate tick).

---

# Phase 3 sessions 3–5 — limits + reduce + fee'd bench SHIPPED (2026-09-16)

**All three slices landed; 123 lib + 4 + 3 integration tests green; fmt/clippy clean; canonical 1M digest re-proven byte-identical (`0xc3bea4a3b66dd253`).**

- **Position limits** (row 54): per-(user, currency) LOCKED-funds caps, gate in `commit` pre-mutation, default uncapped, `cap=0` = freeze, move-top-up pre-check guarantees the release-then-commit path cannot strand an order lockless. 8 ledger + 3 engine tests.
- **`reduceOrder`** (row 55): `Book::reduce` + `Engine::reduce_order`, clamp-to-remaining, full-reduce = exact cancel (dust included), partial keeps queue position (proven via fill order, the book's public surface). Money rule = a fill of the same size (bid: floor cost; ask: exact lots); subadditivity covers any split. 6 engine tests. Reduce *event* (journal record) lands with the journal slice.
- **Fee'd bench mode** (row 56): `LAUNCHPAD_BENCH_FEES="m,t"` bps at engine construction (malformed spec panics — proven); default = byte-identical canonical path. Bench conservation grows the fee-sink term (valid under both schedules); bench digest unchanged and sufficient. Same-session 1M: 0-fee p50 250 = fee'd(100%) p50 250 — written prediction CONFIRMED. Find: at 10/25 bps the fee'd digest EQUALS 0-fee — bench fills floor realistic fees to zero (the floor rule working, not a bug); 100% bps is the disclosed upper-bound exercise of the fee legs.

**Mid-session catches (the discipline again):** a literal `\n` typo landed inside book.rs from a heredoc-style edit — caught by reading the file before compiling; the first position-limit tests used mis-scaled prices (my invented numbers vs the suite's house scales) — rewritten on the existing `gtc()` helper; a rambling split-release test rewritten to assert the lock after each step; one arithmetic slip in a reduce test (free-balance expectation off by the deposit scale) — the engine was right, the test wasn't.

**Phase 3 remaining:** journal + snapshots + replay (the Done-when: replay produces identical state — proven by the test).

---

# Phase 3 sessions 3–5 — position limits, reduceOrder, fee'd bench mode (2026-09-16)

Three slices in the audit's order. Decisions written BEFORE code:

**Slice 1 — position limits.** Per-(user, currency) cap on **locked** funds (the risk being limited is open-order exposure; free funds are deposited wealth). `Ledger::set_position_limit` / `clear_position_limit` / `position_limit` accessor; default uncapped (every existing test/property unchanged); cap checked in `commit` BEFORE any mutation (failure leaves the account untouched, same contract as InsufficientForOrder); new `RiskError::PositionLimitExceeded { currency, cap, would_lock }`. The bid-move top-up path pre-checks the cap explicitly — its release-then-commit sequence must never fail mid-way (a failed commit after release would strand the order lockless); the pre-check replicates the rule via the accessor, with the invariant documented at both sites. cap=0 blocks all commits; deposits/withdrawals unaffected. Tests: reject at cap, boundary-pass, move-into-cap rejected with nothing moved, move-under-cap fine, cap=0, per-account independence, set/clear.

**Slice 2 — reduceOrder** (exchange-core adopt-deferred, row: "decrease by N lots, clamped to remaining, removal when fully reduced"). `Book::reduce(id, by)` — partial reduce keeps queue position (a reduce is not a reprice); full reduction removes via the existing remove path. `Engine::reduce_order(id, by) -> ReduceOutcome { remaining, removed }`: by=0 rejected (`ZeroReduce`); by ≥ remaining ⇒ exact cancel semantics (whole lock incl. dust released, entry gone). Partial money rule mirrors a fill of the same size exactly: bid releases `quote_cost_floor_ticks(by, price)` (dust stays with the remainder until death — same convention as settlement), ask releases `by.lot()` exact. Subadditivity keeps the bid lock ≥ remaining obligation (floor(by·p)+floor((q−by)·p) ≤ floor(q·p) ≤ ceil(q·p)). No fills occur ⇒ no fee legs. Bench generator UNCHANGED (canonical mix + digest frozen at v1.1; reduce reaches workloads only with a future baseline v2).

**Slice 3 — fee'd bench mode.** `LAUNCHPAD_BENCH_FEES="maker,taker"` (bps) read in bench support; default zero = canonical runs byte-identical. `assert_conservation` in support gains the fee-sink term (valid under both schedules). Bench `state_digest` UNCHANGED — and sufficient: conservation pins the per-currency sink given user balances, so two-run digest equality + conservation prove fee'd determinism completely. Fee'd numbers are never compared to v1.1 (different config); the comparison disclosed is same-session 0-fee vs fee'd p50. Prediction (written before the run): fee legs add ~2 map operations per FILL; fills are 4.9% of ops and p50 is set by the move path ⇒ **p50 expected unchanged at 250 ns** (±noise); any claim beyond that needs the numbers to show it.

---

# Phase 3 session 2 — fees slice IMPLEMENTED + verified (2026-09-16)

**Shipped:** `FeeSchedule` (risk.rs — integer bps, validated ≤ 10_000 at construction, split-multiply overflow-proof `fee_of`, floor rounding) + ledger fee sink (`collect_fee` / `fees_collected`, conservation → Σ(free+locked)+Σ(fees)=deposits) + engine fee legs after both settle legs, dispatched on `taker_side` (self-trades pay fees on both receipts — dispatch on the *side*, not user equality, which misfires when both sides are the same user). `Engine::new` = zero schedule (pre-fee byte-identity); `Engine::with_fees` opts in; `fee_schedule()` accessor for the venue/journal.

**Verification:** fmt/clippy clean; **106 lib tests** (6 new fee tests: floor-on-dust, 100%-cap, overflow-exactness, sink mechanics, no-op-zero, validation) + fee'd property twins (conservation/locks/no-cross/determinism under 10/25 bps) all green. **0-fee byte-identity proven:** canonical 1M-op run → digest `0xc3bea4a3b66dd253` and every realized counter identical to v1.1. Engine proptest digest now also folds the fee sink (test-level only; bench digest function untouched).

**The gates caught 3 bugs in my own fresh code before commit** (recorded in decision row 53): (1) the fee edit clobbered `settle`'s payee-credit branch — invisible to new fee tests, flagged instantly by conservation in pre-existing tests; repaired byte-identical from HEAD; (2) overflow test's expected constant 100× off (correct: 18_446_744_073_709_551); (3) first fee-leg draft charged both fees on base lots — the seller's fee must key on the quote-side `quote_amount`.

**Next slice:** position limits (small, layered on `commit`), per the audit's order: fees ✓ → limits → `reduceOrder` → journal/snapshots/replay.

---

# Phase 3 opening — LEDGER AUDIT + fees slice plan (2026-09-16)

## The audit: phases.md Phase 3 scope vs what exists

phases.md says: "balances, position limits, maker/taker fees; disk journal + snapshots + replay. Done when: replay produces identical state — proven by the test." Verdict per item:

- **Balances — already built (Phase 0/1, more than the phase list admits).** `Ledger` free+locked per (user, currency), reserve-at-place (commit/release/settle, all checked, overdraft unrepresentable), ceil-lock/floor-settle with the subadditivity proof, model-vs-ledger conservation property after every op, engine-level Σ(free+locked)=deposits, self-trade returns the lock to free without transfer. **Not a rewrite — fees and limits extend it.**
- **Risk gate — partially built.** Reserve-at-place IS the over-commitment gate. Missing: **position limits** (per-account caps) — small, layered on `commit`.
- **Maker/taker fees — not built.** TODO §5 defers it here with the note it feeds the Phase 5 fee-effect experiments; venue.md imposes no model constraint (grep empty — the choice is ours, to be recorded).
- **Journal + snapshots + replay — not built** (Phase 0 read the concept; §3 line: "build it in Phase 3"). Precedent exists: the engine is deterministic, digested, and the bench two-run machinery is the replay-test shape.
- **`reduceOrder` — not built** (adopt-deferred from the exchange-core re-read).

**Explicitly NOT in the phases.md Phase 3 scope (stay deferred, now said out loud):** margin modes, per-symbol scales (the dusty-fill haircut persists — noted in risk.rs), interest/settlement. **One conflict to resolve:** book.rs's header claims "no self-match prevention — Phase 3 risk-control work," but phases.md's Phase 3 never mentions it. Audit ruling: **self-match prevention defers to Phase 4** (it becomes load-bearing when untrusted strangers share a matching engine — same trigger as uid gating), BUT the fees slice must define self-trade fee treatment now, because fees make a self-trade money-moving (the exchange collects fees from both legs of a user trading with itself).

## The fees slice — the plan (next implementation session's contract)

**Model: fees charged in the RECEIVED asset, deducted from each side's fill proceeds, accumulated in a per-currency fee sink in the ledger.**

- `FeeSchedule { maker_bps, taker_bps }` — integer basis points, validated ≤ 10_000 at construction (a fee can never exceed the value it taxes, so `floor(value × bps / 10_000) ≤ value` structurally — no receipt can go negative from fees).
- Per fill: the resting order is the **maker** (maker_bps), the incoming order is the **taker** (taker_bps) — the book already knows the roles. Each side's fee = floor(received_value × role_bps / 10_000): buyer's fee in base lots, seller's fee in quote ticks.
- **Why received-asset fees:** nobody needs extra lock headroom. The lock covers the delivered asset (unchanged from Phase 0); the fee comes out of what the side receives. The alternative (fee-on-top in quote) would force every bid's place-time lock to include role-dependent fee headroom — but a GTC bid can fill partly as taker (sweep) and partly as maker (rested) — unresolvable cleanly at place time. Received-asset fees dissolve that problem.
- **Ledger change:** fee sink `HashMap<CurrencyId, u64>` + `collect_fee(payee, currency, amount)` (debit the payee's free after settle) + `fees_collected(currency)` accessor. **Conservation extends:** Σ(free+locked) + Σ(fees) = deposits — the property tests and the bench conservation check both grow the fee term.
- **Self-trades pay fees** — no special case: both receipts are fee-deducted to the sink. (The conservation story stays uniform; full self-match *prevention* is Phase 4.)
- **Rounding:** floor, always — the exchange may under-collect on dusty fills, never over-charge (mirrors the ceil-lock/floor-settle asymmetry logic).
- **Determinism:** the schedule is engine configuration; the journal slice will record it in the journal header so replay reconstructs it (decision noted now, implemented with the journal).
- **Benches stay 0-fee** (schedule of 0/0 = today's exact behavior): baseline v1.1 numbers and digests stay comparable, and the disclosure note goes in the methodology. Fee correctness lives in engine + property tests, where it belongs.

**Test plan:** unit — fee math floor cases (incl. 1-tick fills), bps validation (0, 10_000, 10_001 rejected), zero-fee pass-through identical to current behavior; engine — maker vs taker charged correctly in the received asset, partial fills, dust interplay (fee ≤ received always), self-trade fee flow; property — extended conservation (with fee term) after every op, and the ledger model-vs-ledger property grows the sink.

**Slice order for Phase 3:** (1) fees (this plan) → (2) position limits (small) → (3) `reduceOrder` → (4) journal + snapshots + replay (the Done-when).

# Phase 1 engine facade — task list

- [x] Implement `core/src/engine.rs`: Engine, LiveOrder map, place/cancel/move sagas, EngineOutcome/EngineError
- [x] Wire into `lib.rs`
- [x] Unit tests: every saga path (see plan §Tests)
- [x] Property: engine-level conservation + lock sufficiency, widened strategy — **fixed 2026-09-14: `live_ids` was never populated, so cancel/move arms were silent no-ops; now genuinely exercised**
- [x] Gates: build + fmt + clippy -D warnings + all tests (86/86 at session 1)
- [x] Record: decision rows (engine facade, floor settlement, market reserve), weekly log, TODO Phase 1 pointer
- [x] Commit (3695569)

# Phase 1 scope sweep — task list (session 2)

- [x] Scope check: phases.md §1 mandate vs. shipped surface; gaps = engine-level determinism + no-cross, public-API integration test
- [x] Engine-level determinism property (replay digest, two-run equality) — `engine_replay_is_deterministic`
- [x] No-cross re-asserted through the engine after every op + named proptest — `engine_book_never_locks_or_crosses`
- [x] Public-API integration test — `core/tests/engine_lifecycle.rs` (4 tests)
- [x] Gates green: 92 tests (85 lib + 7 integration), fmt / clippy / test / release
- [x] docs/TODO.md Phase 1 section opened (built-so-far + honest remaining list)
- [x] Commit (d69cffe)

# Phase 1 edge-case sweep — task list (session 3)

- [x] Probe id reuse after death: canceled / fully-filled / killed ids — **finding: recycling is accepted (live-only uniqueness, venue-standard); adopted + documented**
- [x] Probe cancel/move vs killed & canceled ids — no-double-release confirmed (errors, balances untouched)
- [x] Property suite widened: `Recycle` command (place under a mostly-dead id) — invariants hold under recycling traffic
- [x] Gates green: 100 tests (93 lib + 7 integration), fmt / clippy / test / release
- [x] Record: decision-log row, docs/TODO.md sweep item ticked, weekly bullet, README count
- [x] Whole-phase review gate (code-review-and-quality) — **approve, no Critical/Required; two optional findings deferred to Phase 2**
- [x] exchange-core re-read against the finished engine surface — **folded forward to Phase 2 (2026-09-14, decision log): its payoff is their benchmark methodology, which is exactly what Phase 2 opens with; the re-read happens where it is used**
- [x] Fluency gate: saga model + settlement-rounding rule, unscripted — **passed 2026-09-14 (user-reported); recorded in docs/TODO.md**
- [x] Commit (365a3f2, then review verdict a576605)

# Phase 1 closure — record (session 4)

- [x] Fluency-gate pass recorded; Phase 1 marked CLOSED in docs/TODO.md
- [x] Phase 2 section opened in docs/TODO.md (setup → measure → optimize, with the guardrails)
- [x] Decision log: closure row + the 09-07 review row restored to chronological position
- [x] Weekly log: closure bullet; README status → Phase 2
- [x] Commit

# Phase 2 benchmark & optimize — task list (opened 2026-09-14)

- [x] exchange-core re-read (the folded Phase 1 item) — **Done 2026-09-15: adopt/skip list written into `docs/research/exchange-core.md` §re-read (4 surface findings: marketable move, reserve-price move gate, `reduceOrder`, uid gating); methodology capture verified against primary sources; decision-log row**
- [x] Re-read docs/benchmarks.md; finalize the workload-mix definition — **Done 2026-09-15:** 9/3/6/82 seeded mix, ~1,000 live / 1,000 accounts / ±375-tick band, defined in `core/benches/support` + published in docs/benchmarks.md §methodology
- [x] `criterion` as core's second dev-dependency (decision-log row) — **Done 2026-09-15:** 0.8.2, `default-features = false`; rust-version 1.85→1.86
- [x] Close the two reproducibility decisions: rust-toolchain.toml pin + release-profile flags (lto, codegen-units) — each with a logged reason — **Done 2026-09-15:** 1.96.0 pin + `lto = "fat"` / `codegen-units = 1`; local-ignores-pin caveat disclosed
- [x] Bench harness in core/benches/: seeded, fixed input set (determinism applies to benchmarks too) — **Done 2026-09-15:** support (generator + mirror + digest), throughput (criterion), latency (harness=false, exact percentiles, two-run digest proof), workload_selfcheck (CI-run generator contract)
- [x] Baseline v1: ops/sec + p50/p99/p99.99, methodology written BEFORE the number — **Done 2026-09-15:** methodology first; p50 291 ns / p99 709–750 / p99.9 ~1.5 µs; mixed ~3.2 M ops/s; p99.99 honestly deferred (sample size insufficient at 50k ops)
- [ ] All 100 tests stay green through every optimization — correctness is the thing Phase 2 protects

## Session: Phase 2 optimization #1 — profile → hypothesis (2026-09-16)

**Profile (attribution shares, macOS `sample` @ 1 ms, two 10 s windows over 20M-op runs of the instrumented release build — samples agree within ~0.5 pp between invocations):** move_order inclusive ~29%; remove_order ~11.5% (short-queue scan + VecDeque remove; memmove visible but minor); per-op timer ~27% (measurement, not engine — excluded per the profiling methodology); hash_one ~5.2%; **BookSide::best ~4.4%**; rest ~3%; Engine::place ~3.2%.

**Hypothesis H1 (the one target this session):** `BookSide::best()` heap-allocates a `Box<dyn Iterator>` per call (via `levels_in_match_order`) — a Phase 0 query-path assumption that the 82%-move workload invalidated, since move_order's crossing gate calls best() every move and the engine's no-cross assert calls it after every op. Replace with direct `BTreeMap` first/last-key access (no allocation, same semantics). Sweeps keep the boxed iterator — matching path, out of scope.

**Written prediction (before the after-run):** p50 250 ns → ~235–248 ns (3–6% down); p99/p99.9 drop by a similar relative share; mixed throughput +2–5%. MUST NOT move: the 1M-op digest (`0xc3bea4a3…`), any counter, any test. If p50 moves <1%, record the negative result — do not retry until it flatters.

**Outcome (recorded — NEGATIVE RESULT):** p50 250→250 ns; digest `0xc3bea4a3…` byte-identical; all counters identical; 100/100 tests green; fmt/clippy clean. Run-2 mean (−6.9%) and p99.9 (−2.9%) sit inside the session's own demonstrated ±15% run-mean noise band (machine hot from profiling) — not evidence. H1 failed its prediction: the ~4.4% profiler share was attribution blur (1 ms sampling + LTO frame blur + allocator reuse making a hot same-size-class box nearly free). The simplification ships with no performance claim (docs/benchmarks.md §profiling, decision-log row). Next session's honest target: `remove_order` ~11.5% — a level-structure change, scoped before any attempt.

# Phase 2 closure (2026-09-16)

- [x] Fluency gate — **PASSED 2026-09-16 (user-declared, self-administered):** all three items answered and checked unscripted per the user (disclosed scope + comparability disclosure; the negative results and what the three experiments proved; digest determinism + the CI rule). Recorded as declared; exact phrasing appendable on request.
- [x] Phase 2 marked CLOSED in docs/TODO.md with the full gates list and baseline v1.1 as the phase's honest number.
- [x] Phase 3 (risk + event sourcing) opened in docs/TODO.md: fees/position-limits on the existing ledger, `reduceOrder`, journal + snapshots + replay (Done-when: replay-proven-identical test; carry-forwards: `reduceOrder`, digest book-depth, bench digest machinery as precedent).
- [x] README status → Phase 3; bench/ tree line now "live".
- [x] Decision-log closure row; weekly-log bullet.
- [x] Commit + push + CI.

# Phase 2 whole-phase review (2026-09-16, code-review-and-quality, five axes)

**Scope:** the full Phase 2 diff (`9a5571f..02c433f`): bench harness (support/throughput/latency/workload_selfcheck), manifests (rust-toolchain pin, workspace profile, criterion dev-dep + explicit `[[bench]]` entries), CI smoke job, book.rs de-box, and the record files. Tests reviewed first (the harness IS Phase 2's test surface: mirror-exactness, conservation, two-run digest equality, mix-sanity floors — all self-asserting).

**Five axes:**
- **Correctness — clean.** Every run validates its own preconditions (mirror/engine agreement, Σ(free+locked)=deposits, realized-mix counters, digest equality asserted not printed); generator error arms panic loudly on generator bugs; the counted `WouldCross` arm documents its invariant proof instead of trusting it; p99.99 sample-gated; mirror swap-remove handles head/tail/same-id cases.
- **Readability — clean.** Every non-obvious decision documented with its WHY in place (per-target module copies, refill rule + the 4.7 ns degenerate story, disclosed scopes, dead-test history). Names carry intent.
- **Architecture — clean.** Cargo pattern correct (support/ not auto-discovered; explicit `[[bench]]` entries); benches touch public API only; engine.rs **not grown** in Phase 2 (Phase 1's optional finding (i) stays dormant — closed as no-action); the reverted arena left **zero debt**: book.rs carries only the `best()` simplification + provenance docs.
- **Security — clean.** CI least-privilege (`contents: read`); dev-only deps, one decision row each, lean criterion features; workspace `unsafe` forbid untouched; no secrets.
- **Performance — the phase's own discipline held.** Methodology-before-numbers twice (baseline, profiling); two negative results recorded instead of kept; no speculative complexity retained; CI correctly barred from publishing numbers (and the bar was proven necessary: 2,845→5,010 ns p99.9 on one runner).

**Verification story:** 100/100 tests at every step; four gates + `bench --no-run` + selfcheck + 2k smoke per session; CI green on every pushed commit; digests cross-checked M1↔x86_64; prediction-before-after-run discipline on all three experiments; the revert executed per the pre-set bar.

**Verdict: APPROVE — no Critical, no Required.** Optional findings (deferred, owner: future harness revision): (1) decompose `moves_rejected` into its three causes (clamped no-op / dead target / WouldCross) — the methodology already discloses the conflation; (2) if a third harness consumer appears, lift `MIN_SAMPLES_P9999`/`DEFAULT_OPS` into support (third-use rule). **Carry-forward to Phase 3 (from Phase 1's review, still open): the replay digest could fold book depth for marginally stronger evidence — relevant again now that bench digests exist alongside it.** Phase 2's Done-when triple is met: harness reproducible, methodology published, honest number recorded.

# Phase 2 optimization session #3 — IMPLEMENTATION of Option A (2026-09-16)

**Design as implemented (vs the scope sketch — one honest revision):** slot arena (`Vec<Slot>` per side, free-list recycling, cap grows on demand) + per-level intrusive doubly-linked chains (`head/tail/count` slot indices) + **the locator index now stores the slot id** (`Locator { side, price, slot }`) so cancel/move unlink O(1) with no level lookup for the splice itself. Level teardown keeps ONE BTreeMap descent (only when a level empties); `rest` keeps the `entry(new)` descent. **Revision vs scope:** the probe's 81–97 ns "move pattern" number hides a cost the scope didn't name — every level teardown/creation is a heap alloc/free of `PriceLevel`+`VecDeque`, and at ~750 levels under 82% moves that churn is constant; the arena kills it too, which is why the honest bound widens slightly rather than shrinks.

**Written prediction (BEFORE the after-run, per the rule):** p50 250 ns → **215–235 ns** (−6% to −14%: moves save scan+deque-remove+get_mut descent+alloc/free churn ≈ 25–45 ns; cancels save ~13–15 ns; places unchanged); p90/p99 improve by a similar relative share; mixed throughput +5–12%. MUST NOT move: the 1M-op digest (`0xc3bea4a3…`), any counter, any of the 100 tests (all unmodified — the property suites are the safety net). **Revert bar (set in session #2):** keep only on ≥5% p50 at 1M ops; 2–4% = revert-and-record; <2% or any correctness wobble = revert-and-record.

**Outcome (recorded — NEGATIVE RESULT, bar fired, REVERTED):** implemented fully (arena + chains + slot locators); the pre-compile mistake-check caught two real chain bugs in my fresh code (stale `next` on moved orders → corruption; emptied-level `tail` reset) — fixed before any run. Then: **100/100 tests unmodified green, digest `0xc3bea4a3…` byte-identical, counters identical — and p50 250→250 ns (run 2: p90 334→333, p99 667→667; mean 283/258 straddles the baseline's noise).** Prediction failed; 0% < 5% bar → reverted (uncommitted; the implementation text is recoverable from this session if ever needed, e.g. at Phase 3/4 scale). **The plateau fact (two structural experiments, one result):** p50 250 ns is insensitive to the book's internal storage — the per-op cost is the ensemble (timer ~27% by the profile, risk/lock checks, index hash, generator bookkeeping) and out-of-order execution hides the tree-op latency inside it; isolated micro-costs do not compose additively in the real stream. The plateau rule ("2 sessions at a plateau → publish the honest number and move on") has now met its condition: sessions #1 and #3 both left p50 unmoved with perfect correctness.

# Phase 2 optimization session #2 — SCOPE (2026-09-16, no code yet)

**Target:** `remove_order` ~14.8% self-time (refined profile, 2×12 s windows @1 ms; `memmove` only 1–3%). Interrogation (questions/Full) + user decisions: path **F-then-A/D**; budget **3–4 more sessions**; revert bar delegated, set below.

**What F found (research + probes, guidance not published evidence):**

- Occupancy fact: GTC places jitter uniformly over ~750 levels → ~1.3 orders/level at steady state. The linear queue scan sees ~1–2 elements.
- Scratch micro-probe (20M iters ×2): one `get_mut` descent @750 levels **11–12 ns**; the full move pattern (remove-old + entry-new teardown/insert) **81–97 ns**; `VecDeque::remove` @len≤2 **~4 ns**.
- ⇒ The 14.8% is **tree descents, not the scan**: the id→locator→level double descent plus teardown descents (~85 ns × 82% ≈ 70 ns of the ~210 ns non-timer path, consistent with the profile). Cross-checked against exchange-core's own note (1,000 orders in ~750 slots).
- **Option B (index-storing locators / swap-remove) — measured dead:** attacks the ~4 ns scan; adds stale-position hazards.
- **Option D (ghost levels) — rejected:** ≤ ~11 ns bound (one teardown descent) vs breaking the "no ghost levels" invariant (documented, test-enforced) — H1's twin risk.
- **Option C (flat price array) — rejected:** requires a PriceOutsideBand rejection = core-semantics change; guardrail bars it in Phase 2.
- **Option A (slot arena + intrusive per-level chains, O(1) unlink/relink) — the only live option:** kills the queue descent AND the double id→locator→level traversal; `entry(new)` + teardown descents remain. Honest bound: **~25–40 ns off p50 (~10–16%)** if the profile share is real.

**Session-2 plan (when executed):** implement A behind the unchanged public `OrderBook` API; property tests (price-time, no-cross, conservation, determinism) stay green unmodified — they are the safety net; canonical 1M before/after with the prediction written first.

**Revert bar (user delegated; set 2026-09-16):** A is kept only if the canonical harness proves **≥5% p50 improvement** at 1M ops (≈ ≥12 ns) with digest/counters/100 tests identical; 2–4% = revert-and-record (not worth structural debt); <2% or any correctness wobble = revert-and-record. Negative results are results: a failed A is recorded and the plateau rule starts its 2-session clock.
