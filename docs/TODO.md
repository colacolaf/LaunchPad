# Phase 0 TODO — Foundations (the build phase)

> **Goal:** go from zero to a minimal order book in Rust with passing unit tests + the language and microstructure fluency to build Phase 1 on top of it.
> **Budget:** ~8 hrs/week, weekends only (Sep 2026). One window, one task. Stop conditions are below.
> **Done when:** you can explain price-time priority in 3 sentences, your tests pass, and the stack decision (Rust, committed 2026-08-21) is reflected in the codebase.
> **Update this file:** check boxes off as you go; log every decision in `docs/record/decision-log.md` and write a weekly entry in `docs/record/weekly-log.md`.

This is the only phase you work on right now. Phases 1–7 live in `docs/phases.md` — do not touch them until Phase 0's stop condition is met.

---

## 0. Pre-flight (open decisions — close these first)

- [x] **Resolve Rust vs Java** (`docs/stack.md`) — **Rust committed (2026-08-21).** The original write-both-then-pick rule was dropped; the deep-research pass confirmed the recommendation, so the user committed to Rust directly. The minimal Rust order book is now the Phase 0 *deliverable* (§5), not a decision input. Java is deferred — only revisit if side-by-side benchmark parity with exchange-core becomes the top priority.
  - [x] ~~Write minimal order book in Rust (place + cancel + best-price match, no tests yet)~~ — moved to §5 as the build deliverable.
  - [x] ~~Write minimal order book in Java (same surface, for comparison)~~ — comparison skipped per the commitment.
  - [x] ~~Compare: learning curve, time-to-working, how far you can reason about it~~ — superseded.
  - [x] Log the decision in `docs/record/decision-log.md` — done (single Adopted row, see `docs/stack.md`).
- [x] **Pick the project name** ("Launchpad" is a working title). Run a quick `questions` Lite on the choice if unsure. Log it. — **Done (2026-08-21): "Launchpad" Adopted.**
- [x] **Decide the license** (TBD in README) before any external dependency or contribution — MIT or Apache-2.0 (exchange-core uses Apache-2.0). Log it. — **Done (2026-08-21): MIT Adopted.**
- [x] Confirm the 12-month record clock is understood: every session → one decision-log line; every week → a weekly-log entry. — **Confirmed 2026-09-05:** scaffold session has its decision-log row + weekly-log entry. Clock is running; no missing weeks.

## 1. Toolchain & repo setup (do this once, then never again)

- [x] Install Rust via rustup (stable toolchain); confirm `cargo --version` and `rustc --version`. — **Done 2026-09-05:** cargo/rustc/clippy/rustfmt all present via Homebrew (rustup absent — noted; matters only if a pinned `rust-toolchain.toml` is adopted later). *Rustup install still recommended so future components are managed consistently.*
- [x] Confirm Python toolchain for the SIM layer later (pyenv/venv, `python --version`); not needed for Phase 0 code but set it up now to avoid a Phase 5 stall. — **Done 2026-09-05:** python 3.14.3 verified; venv creation deferred to Phase 5 (nothing to install yet).
- [x] `cargo init` the workspace; decide crate layout early:
  - [x] `core/` crate — the exchange (order book, matching, risk, event sourcing) — **Done 2026-09-05:** `launchpad-core`, edition 2024, inherits workspace metadata/lints.
  - [x] `bench/` (or criterion in `core/benches`) — the benchmark harness (Phase 2, but scaffold the dir now) — **Done:** dir + README scaffolded; criterion deferred to Phase 2 per §10.
  - [x] `sim/` (Python, Phase 5) — leave a placeholder README — **Done.**
  - [x] `venue/` (Phase 4+) — leave a placeholder README — **Done.**
- [x] Set up a CI workflow (GitHub Actions): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. Badge in README is live from day 1 (the "no fake green" rule). — **Done 2026-09-05:** `.github/workflows/ci.yml` (4 parallel jobs); README badge wired to the real workflow URL.
- [x] Add a smoke-benchmark CI job placeholder (full benchmarks land in Phase 2; keep the job green but minimal). — **Done:** release build + release tests; real numbers only from Phase 2.
- [x] Configure rustfmt + clippy to project-standard; commit `rustfmt.toml` / `clippy.toml` if you set non-defaults. — **Done:** `rustfmt.toml` (style_edition 2024 only); clippy policy centralized in `[workspace.lints]` (no non-defaults to commit).
- [x] Add `.cargo/config.toml` for any target-feature flags later (leave default for now). — **Skipped by decision 2026-09-05:** an empty config file is dead weight; add it when a flag is actually needed. (Revisit only with a logged reason.)
- [x] README badges: wire CI + (later) benchmark badges; mark benchmarks "pending" honestly. — **Done:** CI badge live; benchmarks honestly "pending" until Phase 2.

## 2. Learn Rust (AI agent as tutor — but you must explain every line)

Track against a concrete checklist; "done" = you can do it unaided, not "I watched a video."

### Language fundamentals
- [ ] Ownership, borrowing, lifetimes — the central gate. Be able to explain why a borrow fails without trial-and-error.
- [ ] Move semantics vs `Copy` types; when to `.clone()` and why it's a smell in the hot path.
- [ ] References (`&`, `&mut`) and the aliasing rules.
- [ ] `String` vs `&str`; `Vec<T>` vs slices.
- [ ] Structs, enums, pattern matching (especially for order types / order actions).
- [ ] `Option<T>` and `Result<T, E>`; the `?` operator; never `unwrap` in shipped code (tests OK).
- [ ] Modules, crates, visibility (`pub`, `pub(crate)`); workspace layout.
- [ ] Traits and generics; trait bounds; when generics vs `dyn Trait`.
- [ ] Iterators and the zero-cost-iteration claim — read the assembly once (`cargo asm` / godbolt) to *see* it.
- [ ] Error handling: define a crate `Error` enum early (thiserror); convert with `?`.

### Concurrency & performance (light touch in Phase 0, heavy in Phase 2)
- [ ] `std::sync` basics, `Arc`/`Mutex`/`RwLock`; understand why the hot path will avoid them later.
- [ ] Memory layout intuition: `Vec` vs array, cache-friendliness, why `Box`/heap hurts. (exchange-core's lock-free design lives here — study in §4.)
- [ ] Unsafe: know what it is; do **not** write any in Phase 0.
- [ ] Read `rust-best-practices` (skill) chapters 1–9 alongside the above; apply as you write.

### Cargo & testing
- [ ] `cargo build`/`run`/`test`/`bench`/`doc`/`fmt`/`clippy`/`update` — know each.
- [ ] Built-in `#[test]` unit tests; `#[cfg(test)]` modules.
- [ ] Property-based testing setup (proptest or quickcheck) — needed for order-book invariants.
- [ ] Criterion for micro-benchmarks — set up but don't optimize yet (Phase 2).
- [ ] Test organization: unit tests in-module, integration tests in `tests/`.

## 3. Learn market microstructure (the domain)

"Done" = you can explain each in 1–2 sentences and write a test that encodes it.

- [ ] **Price-time priority** — best price first; at equal price, earliest arrival first. (The spine invariant.)
- [ ] **Bid vs ask**; the **bid-ask spread**; "crossed" and "locked" books (and why they must never happen).
- [ ] **Continuous double auction** — incoming order matches against the opposite side at the best price until filled/exhausted.
- [ ] Order types and their semantics:
  - [ ] **Limit** — rests on the book if unfilled at the limit price.
  - [ ] **GTC** (good-till-cancel) — rests until canceled/filled.
  - [ ] **IOC** (immediate-or-cancel) — fill what you can, cancel the rest.
  - [ ] **FOK** (fill-or-kill) — fill entirely or not at all.
  - [ ] **Market** — take any available price until filled.
- [ ] Order actions: **place, move** (price change without replace — exchange-core prioritizes this, ~0.5µs), **cancel**.
- [ ] **Maker vs taker** fees and why they matter for liquidity (feeds the Phase 5 experiment on fee effects).
- [ ] **No floating point in accounting** — integer scaled values (exchange-core's hard rule; adopt it now). Decide a price/quantity representation early.
- [ ] **Event sourcing basics** — commands → events → journal → snapshot → replay. Read the concept now (build it in Phase 3).
- [ ] Read `docs/research/exchange-core.md` end-to-end; read the latency table and explain its shape.

## 4. Study the reference projects (read, don't copy)

For each: take notes into `docs/research/` (extend the existing notes) with what you'll steal and what you'll skip. **Do not copy code.**

- [ ] **exchange-core** — focus on:
  - [ ] The benchmark methodology (data mix, percentile reporting, hardware disclosure) — copy this discipline verbatim in spirit.
  - [ ] Event-sourcing + snapshot/replay design.
  - [ ] No-floating-point accounting; how they scale prices/quantities.
  - [ ] The place/move/cancel operation model (move as the priority op).
  - [ ] Order types: IOC, GTC, FOK-Budget.
- [ ] **ABIDES** — focus on:
  - [ ] Exchange-agent + many-trading-agents architecture.
  - [ ] Pairwise latency model (every agent↔exchange pair).
  - [ ] ITCH/OUCH-style messaging shape (high-level).
- [ ] **QuantConnect LEAN** — focus on:
  - [ ] Modular plug-in architecture (data/brokerage/execution/risk behind interfaces).
  - [ ] Backtest/live separation (same engine, two modes) — maps to our SIM/VENUE split.
- [ ] Optional: skim the virattt/ai-hedge-fund README to internalize the anti-pattern ("the system does not actually make any trades") — write one line on why Launchpad is the opposite.

## 5. Build the minimal order book (the Phase 0 deliverable)

Correctness first. No optimization. No concurrency. No journal yet. Each item ships with a test.

### Domain model
- [x] Define `Side` (Bid/Ask), `OrderType` (Limit/GTC/IOC/FOK/Market), `OrderAction` (Place/Move/Cancel). — **Done 2026-09-05, with one deviation:** `OrderType` is `{Limit, Market}` × `TimeInForce {Gtc, Ioc, Fok}` instead of five flat variants — Limit/GTC were redundant and Market+GTC is a nonsense state; validity enforced at construction (decision log).
- [x] Decide integer price/quantity representation (no floats); document the scale factors. — **Done:** `PRICE_SCALE = 10_000` (4dp), `QTY_SCALE = 100_000_000` (8dp, satoshi-style); strict decimal-string parsing, zero `f64` in the crate.
- [x] Define `OrderId`, `UserId`, `SymbolId` newtypes. — **Done:** plain `u64` wrappers (`NonZeroU64` noted as a Phase 2 niche-size refinement).
- [x] Define `Order { id, user, side, price, quantity, order_type, timestamp }`. — **Done:** `price` lives inside `OrderType::Limit` (market orders have no price by construction).
- [x] Decide the timestamp source (monotonic counter for determinism — **never wall clock** in the matching path). — **Done:** engine-assigned `u64` sequence on `Order.timestamp`; no clock in the domain.

### Order book
- [x] `OrderBook` struct per symbol. — **Done 2026-09-06:** `BTreeMap<Price, PriceLevel>` per side + FIFO `VecDeque` per level (time priority is queue position); id→locator `HashMap` index for O(log n) cancel/move.
- [x] `place(order)` — insert at correct price level; match against opposite side first. — **Done:** sweep executes at maker prices; GTC remainder rests.
- [x] `cancel(order_id)` — remove from book. — **Done:** returns the removed remaining quantity; unknown/filled ids error.
- [x] `move(order_id, new_price)` — re-insert at new priority (price-time). — **Done:** named `move_order` (keyword); re-queues at tail (priority reset); repricing into the opposite side is rejected (`WouldCross`, decision log).
- [x] Best-bid / best-ask accessors. — **Done:** `best_bid()` / `best_ask()`, O(log P).
- [x] Limit/Market match correctly; IOC cancels remainder; FOK all-or-nothing. — **Done:** FOK is quantity-based (pre-check, never partially executes); exchange-core's FOK-B budget variant noted as a deviation (decision log).

### Matching engine (minimal)
- [x] Continuous double auction on `place`. — **Done 2026-09-06:** the `place` sweep is the auction (engine module separates in a later Phase 0 session).
- [x] Emits fill events (trade records) with price, quantity, maker/taker ids. — **Done:** `Fill { maker_order_id, maker_user, taker_order_id, taker_user, price, quantity }`; execution price is always the maker's.
- [x] Deterministic: same input sequence → same output (a test for this is mandatory). — **Done:** `same_sequence_replays_identically` property (fills + final state compared on a fresh book).

### Risk/accounting (minimal — Phase 3 is the real version)
- [ ] Track per-user balance per currency (integer).
- [ ] Reject orders that would put a user below zero on the quote side (simple balance check).
- [ ] (Defer position limits, margin modes, full fees to Phase 3.)

## 6. Tests (non-negotiable — use the `rust-testing` skill)

### Unit tests
- [x] Place a limit order; it rests; best price updates.
- [x] Place a crossing limit order; it fills; remainder rests or cancels by type.
- [x] IOC fills what it can, cancels the rest (no resting).
- [x] FOK fills entirely or nothing.
- [x] Market order sweeps available liquidity.
- [x] Cancel removes the order; book updates.
- [x] Move changes price and resets time priority. — **All done 2026-09-06 (28 unit tests in `book.rs`, plus domain tests: 42 total).**

### Property-based tests (proptest/quickcheck)
- [x] **Price-time priority invariant:** for any sequence of places at the same price, fills come out in arrival order. — **Done 2026-09-06** (proptest added as the crate's first dev-dependency, decision log).
- [x] **No crossed book:** after any operation, best bid < best ask (or one side empty). — **Done:** asserted after *every* operation in the randomized replay.
- [x] **Conservation:** sum of fills = total traded quantity; no quantity created or destroyed. — **Done:** held *per side* (a crossing trade consumes one lot from each side but records one fill) — the first property run caught both this accounting subtlety and a real index-leak bug (fully-filled makers stayed in the id index; regression-tested).
- [x] **Determinism:** two runs of the same random sequence produce identical state.

### CI gates
- [x] `cargo fmt --check` clean. — **Green locally + in CI run #33991924373; re-proven on every push.**
- [x] `cargo clippy --all-targets -- -D warnings` clean. — **Same.**
- [x] `cargo test` green. — **Same (42/42 at last run).**
- [x] README CI badge live (not a screenshot). — **Live since the first push.**

## 7. Record & review (every week, no exceptions)

- [ ] After each session: one line in `docs/record/decision-log.md` (2 min).
- [ ] Sunday afternoon: a weekly-log entry in `docs/record/weekly-log.md` (built / researched / benchmarks / decided / missed / next week).
- [ ] Run `code-review-and-quality` on the order book before calling Phase 0 done (the gate).
- [ ] Confirm you can explain price-time priority in 3 sentences, cold, unscripted.
- [ ] If you plateaued on a learning item for 2 sessions: write the honest status and move on (the record rewards honesty, not grinding).

## 8. Stretch (only if Phase 0 done-criteria are already met)

- [ ] Add a second order-book implementation alongside the simple one ("Naive" vs "Direct" à la exchange-core) — but don't optimize yet.
- [ ] Write a tiny CLI demo: place a few orders, print the book, print fills.
- [ ] Draft the `docs/research/abides.md` extension with a concrete plan for the Phase 5 agent architecture.

## 9. Phase 0 stop condition (the exit gate)

Phase 0 is **done** when **all** are true:
- [x] Rust-vs-Java decision logged in the decision log. — done (2026-08-21, Rust committed).
- [ ] Minimal order book in Rust (the committed language): place / move / cancel; limit / GTC / IOC / FOK / market.
- [ ] All unit + property tests green (the four invariants: price-time priority, no crossed book, conservation, determinism).
- [ ] CI live and green; README badge live.
- [ ] `code-review-and-quality` review passed.
- [ ] You can explain price-time priority in 3 sentences, unscripted.
- [ ] Weekly log has zero missing weeks for the phase.

If any are false, Phase 0 is not done — do not start Phase 1.

## 10. What is explicitly OUT of scope for Phase 0

- No performance optimization (Phase 2). Do not micro-optimize the book.
- No concurrency / lock-free / LMAX-style pipeline (Phase 2).
- No disk journaling or snapshots (Phase 3).
- No API, WebSocket, or venue (Phase 4).
- No simulator (Phase 5).
- No benchmarks published (Phase 2) — but the harness dir can exist as a scaffold.
