# bench/

Benchmark harness for the CORE layer. **Empty by design in Phase 0** —
`docs/TODO.md` §10 explicitly excludes benchmarks until Phase 2.

This directory exists as a scaffold so the Phase 2 session starts at the
harness, not at repo plumbing.

What lands here (Phase 2 — see `docs/benchmarks.md`):

- **Criterion** micro-benchmarks for the matching engine hot path
  (place / move / cancel; the exchange-core operation model).
- The **published methodology** runner: fixed seed, fixed data mix
  (the exchange-core reference mix: ~9% GTC, 3% IOC, 6% cancel, 82% move),
  percentile reporting (p50/p90/p95/p99/p99.9/p99.99/worst) — never averages.
- The honest-numbers discipline: hardware disclosure, environment fingerprint,
  date. A number without methodology is a claim; with methodology it's evidence.

Until Phase 2, CI runs a **smoke job** (release build + release tests) only —
no benchmark numbers exist, so none are published. No fake green.
