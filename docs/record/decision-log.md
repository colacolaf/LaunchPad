# Decision log

> Append-only. One line per decision/session, ~2 min. Every design choice, every paper trade, every miss. The record starts today.

| Date | Decision | Rationale | Status |
|---|---|---|---|
| 2026-08-21 | Project: Launchpad — matching engine + simulator + hosted competition | Most AI-proof, interview-proof, dual-purpose (portfolio + internships) | Adopted |
| 2026-08-21 | Stack: Rust core + Python sim — committed (write-both-then-pick rule dropped) | Learning-depth is the #1 axis; building from scratch in Rust (no mature Rust reference to copy) deepens learning; compile-time data-race safety + no-GC hot path; strongest interview story. Deep-research pass confirmed the recommendation; the original Phase 0 write-a-minimal-order-book-in-both-Rust-and-Java comparison was judged unnecessary and dropped. Java deferred — only revisit if side-by-side benchmark parity with exchange-core becomes the top priority. | Adopted |
| 2026-08-21 | Public repo from day 1; decision log weekly; quarterly write-ups | The 12-month record (C) is a core deliverable, not a nice-to-have | Adopted |
| 2026-08-21 | Repo scaffolded: AGENTS.md, docs/ (plan, architecture, stack, benchmarks, phases, guardrails, research, record, paper, venue, internships) | Self-contained context for future coding agents; public-safe (no personal data) | Adopted |
| 2026-08-21 | Skills installed in-repo: deep-research, college, questions (personal) + code-review-and-quality, rust-best-practices, rust-testing (ecosystem) | The working skill set for research, planning, and code quality | Adopted |
| 2026-09-05 | Domain model: `OrderType {Limit, Market}` × `TimeInForce {Gtc, Ioc, Fok}` instead of TODO §5's five flat variants | Limit and GTC were redundant as variants, and Market+GTC is a contradiction; the orthogonal pairing lets validity be enforced at construction (`OrderType::for_tif`). Market+FOK rejected for Phase 0 (exotic; revisit Phase 1 if the simulator needs it). TODO wording deviation flagged. | Adopted |
| 2026-09-05 | Scaled-integer constants: `PRICE_SCALE = 10_000` (4dp), `QTY_SCALE = 100_000_000` (8dp, satoshi-style), global for Phase 0 | 8dp matches the finest common subdivision (BTC) so quantity strings rarely need rejecting; per-symbol scales deferred to Phase 3. Strict decimal-string parsing, no `f64` in the crate (exchange-core rule). | Adopted |
| 2026-09-05 | Hand-rolled `DomainError` enum; `thiserror` deferred | Four variants are eyeball-auditable; the dependency lands when the error surface grows — dependency decisions logged one at a time. | Adopted |
| 2026-09-05 | Workspace scaffold: virtual cargo workspace (resolver 3), `core` crate, shared `[workspace.lints]` (unsafe forbid, missing_docs warn), rustfmt style_edition 2024, 4-job CI (fmt/clippy/test/release smoke), `Cargo.lock` committed | One source of truth for metadata+lints; compile-time enforcement of the Phase 0 no-unsafe rule; the four CI jobs mirror the four local gates in TODO §6. Cargo.lock committed for reproducibility (current Cargo guidance overrides the old "ignore for libs" rule). No rust-toolchain.toml pin yet — local cargo is Homebrew, pin would bind CI only; revisit at Phase 2 with benchmark methodology. Empty `.cargo/config.toml` skipped (dead weight until a flag exists). | Adopted |
| — | Benchmark targets final (v2 = 500k ops/sec) | Calibrated to exchange-core reference | Confirm at Phase 2 |

## Template row

| YYYY-MM-DD | [What was decided/done] | [Why — one line] | [Adopted / Confirm at / Rejected] |

## How to add an entry

- Date = today.
- Decision = the thing you chose or did (one line).
- Rationale = why (one line, honest).
- Status = Adopted / Confirm at [phase] / Rejected / Deferred.
- Newest entries go on top of the table (or keep chronological — pick one and stay consistent; chronological is easier to audit).
| 2026-08-21 | Project name: "Launchpad" (lowercase p) | Working title confirmed; docs standardized to "Launchpad". Filesystem dir + GitHub remote are camelCase "LaunchPad" — rename remote repo separately if desired. | Adopted |
| 2026-08-21 | Competition timing | Deferred to Phase 4/5 planning; revisit once the venue exists. Options: Apr–May 2027 (align w/ IMC Prosperity) vs March standalone. | Deferred |
| 2026-08-21 | Paper venue (B): SSRN-style preprint as primary | Preprint fits the anti-vibe-coding, reproducible-first posture; no acceptance gate. 2-3 additional target journals/competitions researched at Phase 7. | Adopted |
| 2026-08-21 | License: MIT | Not commercializing (no path to sell), so no reason to forbid commercial use — permissive license, no friction. MIT over Apache-2.0 for simplicity + consistency with bundled skills; patent clause (Apache) irrelevant for a learning artifact. Matches reference repos being permissive (Apache-2.0 / BSD-3). | Adopted |
