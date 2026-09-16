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
