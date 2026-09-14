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
- [ ] exchange-core re-read against the finished engine surface (adopt/skip list)
- [ ] Whole-phase review gate (code-review-and-quality)
- [ ] Fluency gate: saga model + settlement-rounding rule, unscripted
- [ ] Commit
