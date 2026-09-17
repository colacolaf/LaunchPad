//! The command journal (Phase 3, decision row 59): record the commands an
//! engine accepted — plus every rejection — and replay reproduces the
//! engine's state exactly.
//!
//! **Commands, not events.** The journal stores *inputs* (deposits, places,
//! cancels, reduces, moves — with their acceptance), not state-mutation
//! events. Fills, fee legs, lock movements, and counterparty effects are
//! *derived* by replay through the one and only engine — the alternative
//! (a second, event-applying engine) is a second matching implementation
//! that can drift from the first. Replay identity is therefore proven, not
//! assumed: the same commands through the same engine must end in the same
//! state, checked by [`deep_digest`].
//!
//! **Divergence is a bug, not input.** If a recorded acceptance disagrees
//! with what replay produces ([`JournalError::Divergence`]), either the
//! journal was corrupted or the engine is nondeterministic — both are
//! returned loudly, never papered over (the `Ledger::settle` philosophy).
//!
//! **Scope of this slice (row 59, recorded deferral):** the journal is
//! in-memory. Disk persistence and snapshots are the next increment — the
//! byte format is a pure addition that changes nothing about replay
//! identity, and snapshot-less replay is the *strongest* identity proof
//! (a snapshot fold is weaker than full replay). Phase 3 closes on disk +
//! snapshots, not here.
//!
//! Ground rules unchanged: no floats, no clocks, no `unsafe`, zero deps.

use crate::domain::{CurrencyId, Order, OrderId, Price, Qty, Side, SymbolId, UserId};
use crate::engine::Engine;
use crate::risk::{FeeError, FeeSchedule};

/// Everything the journal layer can reject.
///
/// Hand-rolled like its siblings: small, eyeball-auditable, zero deps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    /// The journal's header does not describe the engine replay was asked
    /// to run against (wrong pair, wrong fees, or no header at all).
    HeaderMismatch {
        /// Journal header pair/fees, as text.
        recorded: String,
        /// The requested engine's pair/fees, as text.
        replayed: String,
    },
    /// Replay disagreed with the recorded acceptance at entry `index`.
    /// This is the phase's divergence alarm: a bug in the engine, the
    /// journal, or a corrupted record — never a valid state.
    Divergence {
        /// 0-based position of the offending entry.
        index: usize,
        /// What the journal recorded.
        recorded: bool,
        /// What replay produced.
        replayed: bool,
    },
    /// The journal header's own fee rate is corrupt (above 10_000 bps): the
    /// recorded configuration cannot build the engine replay must run on.
    CorruptFee(FeeError),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderMismatch { recorded, replayed } => write!(
                f,
                "journal header {recorded} does not match the engine {replayed}"
            ),
            Self::Divergence {
                index,
                recorded,
                replayed,
            } => write!(
                f,
                "replay diverged at entry {index}: recorded accepted={recorded}, replay produced accepted={replayed}"
            ),
            Self::CorruptFee(error) => write!(f, "journal header fee rate is invalid: {error}"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<FeeError> for JournalError {
    fn from(error: FeeError) -> Self {
        // A corrupt header fee rate means the recorded configuration itself
        // fails validation — no engine exists to replay against.
        Self::CorruptFee(error)
    }
}

/// The engine configuration a replay must reconstruct: the pair and the
/// fee schedule. Recorded in the journal header; [`Journal::replay_into`]
/// refuses a mismatch ([`JournalError::HeaderMismatch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalHeader {
    /// The traded symbol.
    pub symbol: SymbolId,
    /// Quote currency (bids lock it, asks receive it).
    pub quote: CurrencyId,
    /// Base currency (asks lock it, bids receive it).
    pub base: CurrencyId,
    /// The fee schedule in force for every fill (decision row 52: the
    /// schedule is engine configuration, so replay must restore it).
    pub fees: FeeSchedule,
}

impl JournalHeader {
    /// Capture the header from a live engine.
    #[must_use]
    pub fn of(engine: &Engine) -> Self {
        Self {
            symbol: engine.symbol(),
            quote: engine.quote(),
            base: engine.base(),
            fees: *engine.fee_schedule(),
        }
    }

    /// The engine this header describes (zero balances, empty book — the
    /// starting state every journal replays from).
    ///
    /// # Errors
    /// [`FeeError`] — a corrupt header fee rate above 10_000 bps.
    pub fn to_engine(&self) -> Result<Engine, FeeError> {
        // Revalidate rather than trust the recorded copy: a corrupt header
        // must not be able to build a schedule the constructor would have
        // rejected. Same rates in → same schedule out.
        FeeSchedule::new(self.fees.maker_bps, self.fees.taker_bps)?;
        Ok(Engine::with_fees(
            self.symbol,
            self.quote,
            self.base,
            self.fees,
        ))
    }

    /// Compact identity string for error messages.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "symbol {:?} quote {:?} base {:?} fees ({} maker, {} taker)",
            self.symbol.0, self.quote.0, self.base.0, self.fees.maker_bps, self.fees.taker_bps
        )
    }
}

/// One command the journal can record — the full input surface of the
/// engine *plus* its external funding operations (deposits/withdrawals are
/// how money enters and leaves; replay cannot reconstruct state without
/// them). `Place` carries the order and the market-bid reserve together:
/// they are one command at the engine's door.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Fund an account (external money in).
    Deposit {
        /// The account's owner.
        user: UserId,
        /// Which currency.
        currency: CurrencyId,
        /// How much.
        amount: u64,
    },
    /// Withdraw from free funds (external money out).
    Withdraw {
        /// The account's owner.
        user: UserId,
        /// Which currency.
        currency: CurrencyId,
        /// How much.
        amount: u64,
    },
    /// Cap an account's locked funds (decision row 54).
    SetPositionLimit {
        /// The capped account.
        user: UserId,
        /// The capped currency.
        currency: CurrencyId,
        /// The cap; 0 is a legal full freeze.
        cap: u64,
    },
    /// Remove a position cap.
    ClearPositionLimit {
        /// The uncapped account.
        user: UserId,
        /// The uncapped currency.
        currency: CurrencyId,
    },
    /// Submit an order (limit/market, GTC/IOC/FOK) with the market-bid
    /// reserve — exactly [`Engine::place`]'s input.
    Place {
        /// The order.
        order: Order,
        /// The market-bid reserve (required for market bids, forbidden
        /// elsewhere).
        reserve: Option<Price>,
    },
    /// Cancel a resting order.
    Cancel {
        /// The order to cancel.
        id: OrderId,
    },
    /// Reduce a resting order's size (the exchange-core `reduceOrder`).
    Reduce {
        /// The order to reduce.
        id: OrderId,
        /// How much to remove; the engine clamps to the remaining amount.
        by: Qty,
    },
    /// Reprice a resting order.
    Move {
        /// The order to reprice.
        id: OrderId,
        /// The new price.
        new_price: Price,
    },
}

/// One journal entry: the command, and whether the engine accepted it.
///
/// Rejections are recorded too (decision row 59, C): replay must agree on
/// *what was refused as well as what was accepted* — a rejection is an
/// observable, deterministic outcome (a risk gate depends only on state),
/// so a disagreement means divergence, not noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The command that was attempted.
    pub command: Command,
    /// Whether the engine accepted it.
    pub accepted: bool,
}

/// The journal: a header (engine configuration) plus the command stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Journal {
    /// The engine configuration the commands ran against.
    pub header: Option<JournalHeader>,
    /// The commands in submission order, each with its acceptance.
    pub entries: Vec<Entry>,
}

impl Journal {
    /// An empty journal with no header.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            header: None,
            entries: Vec::new(),
        }
    }

    /// Begin a journal for `engine`: capture its header.
    #[must_use]
    pub fn for_engine(engine: &Engine) -> Self {
        Self {
            header: Some(JournalHeader::of(engine)),
            entries: Vec::new(),
        }
    }

    /// Run `command` against `engine`, record it with its acceptance, and
    /// return that acceptance — the journal observes, it adds nothing.
    ///
    /// **Infallible by protocol, not by luck.** Every command has a
    /// deterministic outcome — acceptance or a semantic rejection (a risk
    /// gate, an over-free withdrawal, a dead order id) — and outcomes are
    /// exactly what the journal records. A fallible `record` would mean
    /// *unrecorded state change*: replay could never reproduce a stream it
    /// was never handed. Rejections mutate nothing (the ledger and book are
    /// all-or-nothing), so recording `false` is safe.
    pub fn record(&mut self, engine: &mut Engine, command: Command) -> bool {
        let accepted = self.apply(engine, &command);
        self.entries.push(Entry { command, accepted });
        accepted
    }

    /// Run a command without recording it (the replay side of
    /// [`Journal::record`]). Returns whether the engine accepted it.
    /// Infallible: a funding rejection is an outcome like any other (see
    /// [`Journal::record`]).
    pub fn apply(&self, engine: &mut Engine, command: &Command) -> bool {
        match *command {
            Command::Deposit {
                user,
                currency,
                amount,
            } => engine.ledger_mut().deposit(user, currency, amount).is_ok(),
            Command::Withdraw {
                user,
                currency,
                amount,
            } => engine.ledger_mut().withdraw(user, currency, amount).is_ok(),
            Command::SetPositionLimit {
                user,
                currency,
                cap,
            } => {
                engine.ledger_mut().set_position_limit(user, currency, cap);
                true
            }
            Command::ClearPositionLimit { user, currency } => {
                engine.ledger_mut().clear_position_limit(user, currency);
                true
            }
            Command::Place { order, reserve } => engine.place(order, reserve).is_ok(),
            Command::Cancel { id } => engine.cancel(id).is_ok(),
            Command::Reduce { id, by } => engine.reduce_order(id, by).is_ok(),
            Command::Move { id, new_price } => engine.move_order(id, new_price).is_ok(),
        }
    }

    /// Replay this journal into `target`, which must match the header's
    /// configuration. Every entry re-runs through the engine; a recorded
    /// acceptance that disagrees with replay stops the replay loudly.
    ///
    /// # Errors
    /// - [`JournalError::HeaderMismatch`] — `target`'s pair/fees differ
    ///   from the header, or the journal has no header;
    /// - [`JournalError::Divergence`] — a recorded acceptance disagrees
    ///   with replay.
    pub fn replay_into(&self, target: &mut Engine) -> Result<(), JournalError> {
        let header = self
            .header
            .as_ref()
            .ok_or_else(|| JournalError::HeaderMismatch {
                recorded: "(no header)".into(),
                replayed: JournalHeader::of(target).describe(),
            })?;
        if JournalHeader::of(target) != *header {
            return Err(JournalError::HeaderMismatch {
                recorded: header.describe(),
                replayed: JournalHeader::of(target).describe(),
            });
        }
        for (index, entry) in self.entries.iter().enumerate() {
            let replayed = self.apply(target, &entry.command);
            if replayed != entry.accepted {
                return Err(JournalError::Divergence {
                    index,
                    recorded: entry.accepted,
                    replayed,
                });
            }
        }
        Ok(())
    }

    /// Replay into a brand-new engine built from this journal's own header
    /// — the common case: same configuration, empty starting state.
    ///
    /// # Errors
    /// As [`Journal::replay_into`]; a corrupt header fee rate surfaces as
    /// [`JournalError::CorruptFee`].
    pub fn replay(&self) -> Result<Engine, JournalError> {
        let mut engine = self
            .header
            .as_ref()
            .ok_or_else(|| JournalError::HeaderMismatch {
                recorded: "(no header)".into(),
                replayed: "(no header)".into(),
            })?
            .to_engine()?;
        self.replay_into(&mut engine)?;
        Ok(engine)
    }
}

/// Fold a `u64` into the FNV-1a digest.
fn fold_u64(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    // Separator so (a, b) and (b, a) folds can't collide.
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
}

/// The deep replay digest — the Phase 1 carry-forward, landed (decision
/// row 59, D): every observable dimension of the engine's state, folded.
///
/// Folds, in order: header config (pair + fees), all ledger accounts
/// (sorted), the fee sink, position limits, the live-order map (sorted by
/// id), and **book depth on both sides in match order with per-level queue
/// order** — price-time priority is digest-observable, which no aggregate
/// can see.
#[must_use]
pub fn deep_digest(engine: &Engine) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    // Configuration: the pair and the fee schedule.
    let header = JournalHeader::of(engine);
    fold_u64(&mut hash, header.symbol.0);
    fold_u64(&mut hash, header.quote.0);
    fold_u64(&mut hash, header.base.0);
    fold_u64(&mut hash, header.fees.maker_bps);
    fold_u64(&mut hash, header.fees.taker_bps);

    // Money: every account, the fee sink, the caps.
    for (user, currency, free, locked) in engine.accounts() {
        fold_u64(&mut hash, user.0);
        fold_u64(&mut hash, currency.0);
        fold_u64(&mut hash, free);
        fold_u64(&mut hash, locked);
    }
    for (currency, collected) in engine.ledger().fee_sink() {
        fold_u64(&mut hash, currency.0);
        fold_u64(&mut hash, collected);
    }
    for (user, currency, cap) in engine.ledger().position_limits() {
        fold_u64(&mut hash, user.0);
        fold_u64(&mut hash, currency.0);
        fold_u64(&mut hash, cap);
    }

    // Live orders: sorted by id for hash-order independence.
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
        fold_u64(&mut hash, live.price.map_or(0, |price| price.tick()));
        fold_u64(&mut hash, live.remaining.lot());
        fold_u64(&mut hash, live.locked);
    }

    // Book depth: the price-time structure itself, both sides, match
    // order, queue order per level.
    for side in [Side::Bid, Side::Ask] {
        let depth = engine.book().depth(side);
        fold_u64(&mut hash, depth.len() as u64);
        for (price, queue) in depth {
            fold_u64(&mut hash, price.tick());
            fold_u64(&mut hash, queue.len() as u64);
            for (order_id, user, lots) in queue {
                fold_u64(&mut hash, order_id.0);
                fold_u64(&mut hash, user.0);
                fold_u64(&mut hash, lots);
            }
        }
    }

    hash
}

#[cfg(test)]
mod tests;
