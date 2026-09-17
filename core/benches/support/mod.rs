//! Shared benchmark workload — the Phase 2 seeded generator (decision-log
//! row 2026-09-15).
//!
//! One job: turn a fixed seed into a deterministic replica of the reference
//! mix (`docs/benchmarks.md`): 9% GTC place / 3% IOC / 6% cancel / 82% move,
//! ~1,000 live orders, ~800 price levels (the reference's "~750-slot" feel),
//! 1,000 accounts. Determinism is the point — same seed → same command
//! stream → same final state — because the latency harness proves the
//! engine's replay determinism by running the whole stream twice and
//! diffing a state digest.
//!
//! Design notes, each load-bearing:
//! - **Liveness mirror, lazily repaired.** The generator keeps an O(1)
//!   id→(side, price, slot) mirror of what *should* be live. It is synced
//!   exactly on the events the engine reports back (rests, fills' maker
//!   deaths, cancel/move outcomes), never by scanning the book — a scan
//!   per op would cost O(book) and poison the measurement. A cheap
//!   end-of-run check (`assert_mirror_matches_engine`) proves the mirror
//!   never drifted.
//! - **GTC prices jitter outside the spread** (bids strictly below mid,
//!   asks strictly above), so GTC places rest rather than trade and the
//!   trade rate stays IOC-driven — the reference's own no-avalanche
//!   clause. The trade rate this produces is disclosed in
//!   `docs/benchmarks.md`, not tuned to match theirs.
//! - **Moves re-check the BBO** before re-pricing (`engine.book()` is
//!   O(1)) so the engine's `WouldCross` gate is exercised only on genuine
//!   races, not on generator blindness.
//!
//! The generator itself is *not* excluded from the latency measurement:
//! each timed op is the full client-to-ack path (build command + engine
//! call + bookkeeping), which is what a venue's gateway would pay too.
//! That scope is disclosed in the methodology.

// Cargo compiles one private copy of this module PER bench target, so items
// used by only one target (the latency runner's timer/percentiles, the
// counters' audit fields) are legitimately-unused code in the other copy.
// A blanket module allow is the honest fix: it is scoped to bench support
// only, and the crate's #[expect] style cannot express "used by exactly one
// of several includers".
#![allow(dead_code)]
// (The generator's contract tests previously lived in a #[cfg(test)] module
// here — dead code: cargo's test harness never sees harness=false bench
// dependencies. They now run for real in the workload_selfcheck target.)

use std::collections::HashMap;
use std::time::Instant;

use launchpad_core::book::BookError;
use launchpad_core::domain::{
    CurrencyId, Order, OrderId, OrderType, Price, Qty, Side, SymbolId, TimeInForce, UserId,
};
use launchpad_core::engine::{Engine, EngineError, EngineOutcome};
use launchpad_core::risk::FeeSchedule;

/// The one traded symbol (Phase 2 benches are single-symbol, per the
/// reference methodology).
pub(crate) const SYMBOL: SymbolId = SymbolId(1);
/// Quote currency for the bench ledger.
pub(crate) const USD: CurrencyId = CurrencyId(1);
/// Base currency for the bench ledger.
pub(crate) const BTC: CurrencyId = CurrencyId(2);

/// Number of participant accounts (reference: 1,000).
pub(crate) const USERS: u64 = 1_000;
/// Live orders the steady-state preload builds (reference: ~1,000).
pub(crate) const PRELOAD_ORDERS: usize = 1_000;
/// Book band around the mid: ±375 ticks ≈ 750 price levels, the
/// reference's "~750 slots".
pub(crate) const BAND_HALF_TICKS: u64 = 375;
/// Mid price: 100.0 quote units (PRICE_SCALE = 10_000 ticks/unit).
pub(crate) const MID_TICKS: u64 = 1_000_000;
/// Per-account deposit, quote side: 100_000.0 USD — deep enough that risk
/// rejections are structurally absent (max commitment ≈ 100 lots ×
/// 1_000_375 ticks ≈ 0.1 units). The risk gate is exercised elsewhere; the
/// bench isolates matching cost.
pub(crate) const USD_DEPOSIT: u64 = 1_000_000_000_000;
/// Per-account deposit, base side: 10_000.0 BTC.
pub(crate) const BTC_DEPOSIT: u64 = 1_000_000_000_000;

// IOC asks stay inside the band even after jitter subtraction.
const BAND_FLOOR: u64 = MID_TICKS - BAND_HALF_TICKS;

// ---- Reference mix (percent rolls; 9 + 3 + 6 + 82 = 100) ------------------
const GTC_ROLL: u64 = 9;
const IOC_ROLL: u64 = 12; // +3
const CANCEL_ROLL: u64 = 18; // +6
// everything above CANCEL_ROLL (82%) is a move.

/// splitmix64 — tiny, dependency-free, fully deterministic. `below`'s
/// modulo bias is irrelevant at bench resolution and is disclosed in the
/// methodology (the stream is fixed per seed either way).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish value in `[0, n)`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// Value in `[lo, hi]` inclusive.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.below(hi - lo + 1)
    }
}

/// What the generator believes is live, keyed by order id. `slot` indexes
/// the id into [`Workload::ids`] so cancels/moves can pick a uniformly
/// random live id in O(1) and removals stay O(1) via swap-remove.
struct MirrorOrder {
    side: Side,
    price_ticks: u64,
    slot: usize,
}

/// The counters every bench run prints — the numbers that make a run
/// auditable (a mix that silently produced zero trades would invalidate
/// every percentile in the table).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Counters {
    /// GTC places that returned `Ok` (rested or fully filled).
    pub(crate) gtc_places: u64,
    /// IOC places that returned `Ok`.
    pub(crate) ioc_places: u64,
    /// Cancels the engine accepted.
    pub(crate) cancels_ok: u64,
    /// Cancels rejected with `UnknownOrder` — the target had already died
    /// by fill; the mirror is repaired and a fresh target is implied next
    /// roll. A *high* rate here would mean the mirror is stale.
    pub(crate) cancels_missed: u64,
    /// Moves the engine accepted.
    pub(crate) moves_ok: u64,
    /// Moves rejected (`WouldCross` / `UnknownOrder`).
    pub(crate) moves_rejected: u64,
    /// Individual trades (fills).
    pub(crate) trades: u64,
    /// Traded volume, raw lots.
    pub(crate) volume_lots: u64,
}

/// The seeded command stream + liveness mirror. `step` executes exactly one
/// command against the engine, sampled from the reference mix.
pub(crate) struct Workload {
    rng: Rng,
    next_id: u64,
    timestamp: u64,
    ids: Vec<u64>,
    live: HashMap<u64, MirrorOrder>,
    counters: Counters,
}

impl Workload {
    pub(crate) fn new(seed: u64) -> Self {
        Workload {
            rng: Rng::new(seed),
            next_id: 0,
            timestamp: 0,
            ids: Vec::with_capacity(PRELOAD_ORDERS * 2),
            live: HashMap::with_capacity(PRELOAD_ORDERS * 2),
            counters: Counters::default(),
        }
    }

    pub(crate) fn counters(&self) -> Counters {
        self.counters
    }

    /// How many orders the generator currently believes live (mirror len —
    /// O(1)). The throughput chunks use this to hold the book at its
    /// steady-state size while driving a draining arm (cancel removes
    /// orders; IOC fills kill makers).
    pub(crate) fn live_len(&self) -> usize {
        self.ids.len()
    }

    /// Fresh engine with all 1,000 accounts funded on both legs.
    ///
    /// Fee schedule (Phase 3, decision row 56): `LAUNCHPAD_BENCH_FEES`
    /// = `"maker,taker"` in basis points opts the run into fees — e.g.
    /// `LAUNCHPAD_BENCH_FEES=10,25`. Unset (or unparseable) keeps the zero
    /// schedule, so canonical runs stay byte-identical to baseline v1.1.
    /// Fee'd runs are a *different configuration*: their numbers are never
    /// compared against the canonical baseline, only against a 0-fee run
    /// of the same session (the disclosure says so).
    pub(crate) fn fresh_engine() -> Engine {
        let mut engine = match std::env::var("LAUNCHPAD_BENCH_FEES") {
            Ok(spec) => {
                let (maker, taker) = spec
                    .split_once(',')
                    .and_then(|(m, t)| {
                        m.trim()
                            .parse::<u64>()
                            .ok()
                            .zip(t.trim().parse::<u64>().ok())
                    })
                    .unwrap_or_else(|| {
                        panic!("LAUNCHPAD_BENCH_FEES must be \"maker,taker\" in bps, got {spec:?}")
                    });
                let schedule = FeeSchedule::new(maker, taker)
                    .unwrap_or_else(|e| panic!("invalid LAUNCHPAD_BENCH_FEES: {e}"));
                Engine::with_fees(SYMBOL, USD, BTC, schedule)
            }
            Err(_) => Engine::new(SYMBOL, USD, BTC),
        };
        for user in 0..USERS {
            let user = UserId(user);
            engine.ledger_mut().deposit(user, USD, USD_DEPOSIT).unwrap();
            engine.ledger_mut().deposit(user, BTC, BTC_DEPOSIT).unwrap();
        }
        engine
    }

    /// Build the steady state: PRELOAD_ORDERS GTC resting orders via the
    /// same `place` path the mix uses, so the mirror starts exact.
    pub(crate) fn bootstrap(&mut self, engine: &mut Engine) {
        for _ in 0..PRELOAD_ORDERS {
            self.step_gtc(engine);
        }
    }

    /// Execute one command sampled from the reference mix.
    pub(crate) fn step(&mut self, engine: &mut Engine) {
        let roll = self.rng.below(100);
        if roll < GTC_ROLL {
            self.step_gtc(engine);
        } else if roll < IOC_ROLL {
            self.step_ioc(engine);
        } else if roll < CANCEL_ROLL {
            self.step_cancel(engine);
        } else {
            self.step_move(engine);
        }
    }

    fn random_user(&mut self) -> UserId {
        UserId(self.rng.below(USERS))
    }

    fn random_qty(&mut self) -> Qty {
        Qty::from_lots(self.rng.range(1, 100)).unwrap()
    }

    /// GTC arm in isolation (throughput's per-op chunk): prices jitter
    /// *outside* the mid (bids ≤ mid−1, asks ≥ mid+1), so GTCs almost
    /// always rest and the trade rate stays IOC-driven. Precise edge case:
    /// a move may previously have dragged the BBO across the mid, in which
    /// case a fresh GTC taker-fills — a valid engine outcome the mirror
    /// absorbs (resting `None` → never inserted). The only unreachable
    /// rejections are duplicate/symbol/overflow/risk, asserted below.
    pub(crate) fn step_gtc(&mut self, engine: &mut Engine) {
        let side = if self.rng.below(2) == 0 {
            Side::Bid
        } else {
            Side::Ask
        };
        let price_ticks = match side {
            Side::Bid => self.rng.range(MID_TICKS - BAND_HALF_TICKS, MID_TICKS - 1),
            Side::Ask => self.rng.range(MID_TICKS + 1, MID_TICKS + BAND_HALF_TICKS),
        };
        let id = self.next_id;
        let order = Order::new(
            OrderId(id),
            self.random_user(),
            SYMBOL,
            side,
            OrderType::Limit {
                price: Price::from_ticks(price_ticks).unwrap(),
            },
            TimeInForce::Gtc,
            self.random_qty(),
            self.timestamp,
        )
        .unwrap();
        self.next_id += 1;
        self.timestamp += 1;

        match engine.place(order, None) {
            Ok(outcome) => {
                self.counters.gtc_places += 1;
                self.absorb_outcome(engine, &outcome);
                if let EngineOutcome::Executed {
                    resting: Some(_), ..
                } = outcome
                {
                    self.insert_live(id, side, price_ticks);
                }
            }
            // Fresh ids + never-crossing prices + deep funds: nothing else
            // is representable. A hit here means the generator is wrong,
            // not the engine — fail loudly rather than publish bad numbers.
            Err(error) => panic!("gtc place rejected unexpectedly: {error}"),
        }
    }

    /// IOC arm in isolation: priced *through* the opposite best (best ±
    /// jitter), so it sweeps 1..a few levels. Never rests, never enters
    /// the mirror.
    pub(crate) fn step_ioc(&mut self, engine: &mut Engine) {
        let side = if self.rng.below(2) == 0 {
            Side::Bid
        } else {
            Side::Ask
        };
        let book = engine.book();
        let price_ticks = match side {
            Side::Bid => match book.best_ask() {
                Some(ask) => ask.tick() + self.rng.below(50),
                None => MID_TICKS,
            },
            Side::Ask => match book.best_bid() {
                Some(bid) => bid
                    .tick()
                    .saturating_sub(self.rng.below(50))
                    .max(BAND_FLOOR),
                None => MID_TICKS,
            },
        };
        let order = Order::new(
            OrderId(self.next_id),
            self.random_user(),
            SYMBOL,
            side,
            OrderType::Limit {
                price: Price::from_ticks(price_ticks).unwrap(),
            },
            TimeInForce::Ioc,
            self.random_qty(),
            self.timestamp,
        )
        .unwrap();
        self.next_id += 1;
        self.timestamp += 1;

        match engine.place(order, None) {
            Ok(outcome) => {
                self.counters.ioc_places += 1;
                self.absorb_outcome(engine, &outcome);
            }
            Err(error) => panic!("ioc place rejected unexpectedly: {error}"),
        }
    }

    /// Cancel arm in isolation: uniform random live id in O(1) via the
    /// slot index. A miss (`UnknownOrder`) means the fill path killed it
    /// after the mirror was written — repair the mirror and count the miss.
    pub(crate) fn step_cancel(&mut self, engine: &mut Engine) {
        debug_assert_eq!(self.ids.len(), self.live.len(), "mirror diverged");
        if self.ids.is_empty() {
            return;
        }
        let slot = self.rng.below(self.ids.len() as u64) as usize;
        let id = self.ids[slot];
        match engine.cancel(OrderId(id)) {
            Ok(_) => {
                self.counters.cancels_ok += 1;
                self.remove_live(id);
            }
            Err(EngineError::Book(BookError::UnknownOrder { .. })) => {
                self.counters.cancels_missed += 1;
                self.remove_live(id);
            }
            Err(error) => panic!("cancel rejected unexpectedly: {error}"),
        }
    }

    /// Move arm in isolation: re-price a random live order by ±1..25
    /// ticks, clamped to the band *and* to non-crossing against the current
    /// BBO. Own-side collisions are legal (price-time priority re-queues);
    /// only the opposite side constrains.
    pub(crate) fn step_move(&mut self, engine: &mut Engine) {
        debug_assert_eq!(self.ids.len(), self.live.len(), "mirror diverged");
        if self.ids.is_empty() {
            return;
        }
        let slot = self.rng.below(self.ids.len() as u64) as usize;
        let id = self.ids[slot];
        let (side, old_ticks) = {
            let mirror = &self.live[&id];
            (mirror.side, mirror.price_ticks)
        };
        // Re-price by ±1..25 ticks: up via checked add (band ceiling makes
        // overflow unreachable, but checked keeps it that way by proof, not
        // luck), down via saturating sub.
        let delta = self.rng.range(1, 25);
        let up = self.rng.below(2) == 0;
        let raw = if up {
            old_ticks
                .checked_add(delta)
                .expect("band ceiling bounds ticks")
        } else {
            old_ticks.saturating_sub(delta)
        };
        // Then clamp against the opposite BBO so the move rests rather than
        // crosses — the engine's WouldCross gate stays exercised only on
        // genuine violations, matching the reference's reserve-price design.
        let book = engine.book();
        let new_ticks = match side {
            Side::Bid => {
                let cap = book
                    .best_ask()
                    .map_or(MID_TICKS + BAND_HALF_TICKS, |a| a.tick());
                raw.min(cap - 1)
            }
            Side::Ask => {
                let floor = book
                    .best_bid()
                    .map_or(MID_TICKS - BAND_HALF_TICKS, |b| b.tick());
                raw.max(floor + 1)
            }
        }
        .clamp(MID_TICKS - BAND_HALF_TICKS, MID_TICKS + BAND_HALF_TICKS);
        if new_ticks == old_ticks {
            // Clamped into a no-op (e.g. order at the band edge). Count it
            // honestly instead of pretending a move happened.
            self.counters.moves_rejected += 1;
            return;
        }

        match engine.move_order(OrderId(id), Price::from_ticks(new_ticks).unwrap()) {
            Ok(()) => {
                self.counters.moves_ok += 1;
                self.live.get_mut(&id).unwrap().price_ticks = new_ticks;
            }
            Err(EngineError::Book(BookError::UnknownOrder { .. })) => {
                // Died by fill between our mirror write and now — repair.
                self.counters.moves_rejected += 1;
                self.remove_live(id);
            }
            Err(EngineError::Book(BookError::WouldCross { .. })) => {
                // Provable dead end, deliberately armed anyway: the only
                // way new_ticks could cross is best_ask < band floor while
                // this bid rests there — a resting CROSSED book, which the
                // engine's Phase 0 invariant forbids. Proving that takes
                // the invariant plus the band-floor argument, so the arm
                // counts the rejection instead of trusting the proof: a
                // counted anomaly is honest data; a panic is a dead run.
                self.counters.moves_rejected += 1;
            }
            Err(error) => panic!("move rejected unexpectedly: {error}"),
        }
    }

    /// Absorb one place outcome: count trades, and drop makers the engine
    /// reports as dead. O(fills) with O(1) liveness checks against the
    /// engine — never an O(book) scan.
    fn absorb_outcome(&mut self, engine: &Engine, outcome: &EngineOutcome) {
        let EngineOutcome::Executed { fills, .. } = outcome else {
            return;
        };
        self.counters.trades += fills.len() as u64;
        for fill in fills {
            self.counters.volume_lots += fill.quantity.lot();
            // A maker at the sweep's frontier can be PARTIALLY filled and
            // still live — only the engine knows which. Ask it (O(1) map
            // lookup per fill): dead ids are unmirrored, survivors stay.
            if !engine.live_orders().contains_key(&fill.maker_order_id) {
                self.remove_live(fill.maker_order_id.0);
            }
        }
        // A resting remainder is mirrored by the caller (step_gtc) — IOC
        // takers never rest, so there is nothing to record here.
    }

    fn insert_live(&mut self, id: u64, side: Side, price_ticks: u64) {
        let slot = self.ids.len();
        self.ids.push(id);
        self.live.insert(
            id,
            MirrorOrder {
                side,
                price_ticks,
                slot,
            },
        );
    }

    fn remove_live(&mut self, id: u64) {
        if let Some(mirror) = self.live.remove(&id) {
            let last = self.ids.len() - 1;
            let moved = self.ids[last];
            self.ids[mirror.slot] = moved;
            self.ids.pop();
            if moved != id {
                self.live.get_mut(&moved).unwrap().slot = mirror.slot;
            }
        }
    }

    /// End-of-run integrity: the mirror must agree with the engine exactly
    /// — same live count, and every mirrored id actually resting. A
    /// failure here invalidates the run's numbers.
    pub(crate) fn assert_mirror_matches_engine(&self, engine: &Engine) {
        assert_eq!(
            self.live.len(),
            engine.live_orders().len(),
            "mirror/engine live-count divergence"
        );
        assert_eq!(
            self.ids.len(),
            engine.book().len(),
            "mirror/book divergence"
        );
        for id in &self.ids {
            assert!(
                engine.live_orders().contains_key(&OrderId(*id)),
                "mirror id {id} is not live in the engine"
            );
        }
    }
}

/// Σ(free + locked) + Σ(fees) must equal total deposits, per currency —
/// the conservation check from the outside (no engine internals). The fee
/// term is zero under the default schedule and equals the exchange's take
/// under `LAUNCHPAD_BENCH_FEES` — valid under both (decision row 56).
pub(crate) fn assert_conservation(engine: &Engine) {
    for (currency, deposit) in [(USD, USD_DEPOSIT), (BTC, BTC_DEPOSIT)] {
        let total: u64 = (0..USERS)
            .map(|u| {
                let user = UserId(u);
                engine.ledger().free(user, currency) + engine.ledger().locked(user, currency)
            })
            .sum::<u64>()
            + engine.ledger().fees_collected(currency);
        assert_eq!(
            total,
            USERS * deposit,
            "conservation broke for currency {currency:?}"
        );
    }
}

/// FNV-1a fold of the engine's *entire* observable state: the live-order
/// table (sorted by id) and every account's free/locked balance on both
/// legs. Two runs of the same seeded stream must produce the same digest —
/// that equality is the replay-determinism proof at bench scale.
pub(crate) fn state_digest(engine: &Engine) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    fn fold(hash: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        // Separator so (a,b) and (b,a) folds can't collide.
        *hash ^= 0xff;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    fn fold_u64(hash: &mut u64, value: u64) {
        fold(hash, &value.to_le_bytes());
    }

    let mut ids: Vec<u64> = engine.live_orders().keys().map(|id| id.0).collect();
    ids.sort_unstable();
    fold_u64(&mut hash, ids.len() as u64);
    for id in ids {
        let live = &engine.live_orders()[&OrderId(id)];
        fold_u64(&mut hash, id);
        fold_u64(&mut hash, live.user.0);
        fold_u64(
            &mut hash,
            match live.side {
                Side::Bid => 0,
                Side::Ask => 1,
            },
        );
        fold_u64(&mut hash, live.price.map_or(0, |p| p.tick()));
        fold_u64(&mut hash, live.remaining.lot());
        fold_u64(&mut hash, live.locked);
        fold_u64(&mut hash, live.currency.0);
    }
    for u in 0..USERS {
        let user = UserId(u);
        fold_u64(&mut hash, engine.ledger().free(user, USD));
        fold_u64(&mut hash, engine.ledger().locked(user, USD));
        fold_u64(&mut hash, engine.ledger().free(user, BTC));
        fold_u64(&mut hash, engine.ledger().locked(user, BTC));
    }
    hash
}

/// Exact sorted-vector percentile (`p` in 0..=100). Not HDR-histogram
/// precision — the methodology says so explicitly.
pub(crate) fn percentile(sorted: &[u64], p: f64) -> u64 {
    let n = sorted.len();
    assert!(!sorted.is_empty(), "percentile of empty samples");
    let idx = ((p / 100.0) * n as f64).ceil() as usize;
    sorted[idx.clamp(1, n) - 1]
}

/// Wall-clock per-op timer, exposed so both benches time identically.
pub(crate) struct OpTimer {
    start: Instant,
}

impl OpTimer {
    pub(crate) fn start() -> Self {
        OpTimer {
            start: Instant::now(),
        }
    }

    pub(crate) fn elapsed_nanos(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
}
