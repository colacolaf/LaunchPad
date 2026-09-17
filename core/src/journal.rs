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
//! **Disk + snapshots landed (decision row 61):** the byte format is a pure
//! framing of the same command stream — magic + header + length-prefixed
//! entries, decode total (corruption is `Err`, never a panic). A
//! [`Snapshot`] captures the engine's full state at a journal position;
//! [`Journal::replay_from`] composes recovery as restore-then-replay-the-
//! tail, proven equal to full replay by the deep digest. Snapshot-less
//! replay remains the *strongest* identity proof (a snapshot fold is weaker
//! than full replay) — which is exactly why the snapshot path is checked
//! against it, not trusted.
//!
//! Ground rules unchanged: no floats, no clocks, no `unsafe`, zero deps.

use crate::domain::{
    CurrencyId, Order, OrderId, OrderType, Price, Qty, Side, SymbolId, TimeInForce, UserId,
};
use crate::engine::{Engine, Snapshot};
use crate::risk::{FeeError, FeeSchedule};

/// Everything the journal layer can reject.
///
/// Hand-rolled like its siblings: small, eyeball-auditable, zero deps.
/// The `Io` variant carries [`std::io::ErrorKind`] rather than the error
/// itself: kinds are `Clone + Eq + Debug` for free, and the OS-level detail
/// belongs to the caller's file layer, not the core's error type.
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
    /// The engine refused a snapshot restore (non-fresh target, or an
    /// internally inconsistent snapshot — locks ≠ Σ order locks, crossed
    /// book, unknown rows). Corruption is returned, never smoothed over.
    Restore(crate::engine::EngineError),
    /// The byte stream is not a valid journal (bad magic, truncated frame,
    /// unknown tag, invalid value, or trailing garbage). Decoding is total:
    /// corruption is an `Err`, never a panic and never silent.
    Corrupt,
    /// The underlying file I/O failed (carries the [`std::io::ErrorKind`]).
    Io(std::io::ErrorKind),
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
            Self::Restore(error) => write!(f, "snapshot restore refused: {error}"),
            Self::Corrupt => write!(f, "journal byte stream is corrupt"),
            Self::Io(error) => write!(f, "journal i/o failed: {error}"),
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

/// A captured engine state paired with the journal position it belongs to
/// (the journal's snapshot pairing, decision row 61): the state existed
/// after exactly `up_to` recorded commands, so recovery composes as
/// restore-then-replay-the-tail — [`Journal::replay_from`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalSnapshot {
    /// The captured engine state (header-consistent with its engine).
    pub state: Snapshot,
    /// How many commands of the journal produced this state.
    pub up_to: usize,
}

impl Journal {
    /// Magic + version: `LPDJ0001` (Launchpad Disk Journal, format 1). A
    /// reader that sees anything else refuses — never guesses.
    pub const MAGIC: [u8; 8] = *b"LPDJ0001";

    /// Encode the journal to bytes: magic + header + entry count + framed
    /// entries. The header rides in the file (flag-prefixed: absent only for
    /// a bare `Journal::new`) so a loaded journal can `replay()` without its
    /// original engine. Hand-rolled (zero deps): every field is a
    /// little-endian `u64`, every entry is length-prefixed, and decode is
    /// **total** — any corruption surfaces as [`JournalError::Corrupt`],
    /// never a panic.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&Self::MAGIC);
        match &self.header {
            None => put_u64(&mut out, 0),
            Some(header) => {
                put_u64(&mut out, 1);
                put_u64(&mut out, header.symbol.0);
                put_u64(&mut out, header.quote.0);
                put_u64(&mut out, header.base.0);
                put_u64(&mut out, header.fees.maker_bps);
                put_u64(&mut out, header.fees.taker_bps);
            }
        }
        out.extend_from_slice(&(self.entries.len() as u64).to_le_bytes());
        for entry in &self.entries {
            let payload = encode_entry(entry);
            out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
            out.extend_from_slice(&payload);
        }
        out
    }

    /// Decode a journal from bytes (the inverse of [`Journal::to_bytes`]).
    ///
    /// # Errors
    /// [`JournalError::Corrupt`] — wrong magic, truncated frame, unknown
    /// command tag, an invalid encoded value (zero price/quantity, an
    /// order-type/time-in-force mismatch), or trailing garbage (the entry
    /// count disagrees with the remaining bytes).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, JournalError> {
        if bytes.len() < 16 || bytes[..8] != Self::MAGIC {
            return Err(JournalError::Corrupt);
        }
        let mut cursor = Cursor { bytes, at: 8 };
        let header = match cursor.take_u64()? {
            0 => None,
            1 => Some(JournalHeader {
                symbol: SymbolId(cursor.take_u64()?),
                quote: CurrencyId(cursor.take_u64()?),
                base: CurrencyId(cursor.take_u64()?),
                fees: FeeSchedule::new(cursor.take_u64()?, cursor.take_u64()?)
                    .map_err(|_| JournalError::Corrupt)?,
            }),
            _ => return Err(JournalError::Corrupt),
        };
        let count = cursor.take_u64()? as usize;
        let mut entries = Vec::with_capacity(count.min(1 << 20));
        for _ in 0..count {
            let len = cursor.take_u64()? as usize;
            if len == 0 || cursor.bytes.len() < cursor.at + len {
                return Err(JournalError::Corrupt);
            }
            let payload = &cursor.bytes[cursor.at..cursor.at + len];
            cursor.at += len;
            entries.push(decode_entry(payload)?);
        }
        // Trailing garbage: the count said the journal ended here.
        if cursor.at != cursor.bytes.len() {
            return Err(JournalError::Corrupt);
        }
        Ok(Self { header, entries })
    }

    /// Write the journal to `path` ([`Journal::to_bytes`], then the file).
    ///
    /// # Errors
    /// Any I/O error surfaced as [`JournalError::Io`].
    pub fn to_file(&self, path: &std::path::Path) -> Result<(), JournalError> {
        std::fs::write(path, self.to_bytes()).map_err(|e| JournalError::Io(e.kind()))
    }

    /// Read a journal from `path` ([`Journal::from_bytes`] on the file).
    ///
    /// # Errors
    /// [`JournalError::Io`] or [`JournalError::Corrupt`] as from
    /// [`Journal::from_bytes`].
    pub fn from_file(path: &std::path::Path) -> Result<Self, JournalError> {
        let bytes = std::fs::read(path).map_err(|e| JournalError::Io(e.kind()))?;
        Self::from_bytes(&bytes)
    }

    /// Capture `engine`'s full state against this journal's current command
    /// position. The engine must be the same one the commands ran against
    /// (same header) — [`Journal::replay_from`] re-checks that.
    #[must_use]
    pub fn snapshot_at(&self, engine: &Engine) -> JournalSnapshot {
        JournalSnapshot {
            state: engine.capture_snapshot(),
            up_to: self.entries.len(),
        }
    }

    /// Restore `snapshot.state` into `target`, then replay only the journal's
    /// trailing commands (`entries[snapshot.up_to..]`) — the composed
    /// recovery path. Proven equal to full replay by the deep digest in the
    /// tests: the snapshot must land the engine in exactly the state the
    /// prefix produced, or the trailing replay diverges loudly.
    ///
    /// # Errors
    /// - [`JournalError::HeaderMismatch`] — target or snapshot disagree with
    ///   this journal's header;
    /// - [`JournalError::Restore`] — the engine refused the snapshot
    ///   (inconsistent or non-fresh target);
    /// - [`JournalError::Divergence`] — a trailing command's replayed
    ///   acceptance disagrees with the record.
    pub fn replay_from(
        &self,
        target: &mut Engine,
        snapshot: &JournalSnapshot,
    ) -> Result<(), JournalError> {
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
        if snapshot.up_to > self.entries.len() {
            return Err(JournalError::Divergence {
                index: snapshot.up_to,
                recorded: false,
                replayed: false,
            });
        }
        target
            .restore_snapshot(&snapshot.state)
            .map_err(JournalError::Restore)?;
        for (index, entry) in self.entries[snapshot.up_to..].iter().enumerate() {
            let replayed = self.apply(target, &entry.command);
            if replayed != entry.accepted {
                return Err(JournalError::Divergence {
                    index: snapshot.up_to + index,
                    recorded: entry.accepted,
                    replayed,
                });
            }
        }
        Ok(())
    }
}

// ---- The disk codec (decision row 61): framed little-endian u64s. ---------

/// Command tags, in [`Command`] variant order.
const TAG_DEPOSIT: u64 = 0;
const TAG_WITHDRAW: u64 = 1;
const TAG_SET_LIMIT: u64 = 2;
const TAG_CLEAR_LIMIT: u64 = 3;
const TAG_PLACE: u64 = 4;
const TAG_CANCEL: u64 = 5;
const TAG_REDUCE: u64 = 6;
const TAG_MOVE: u64 = 7;

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// A decoding cursor: the one place that can say "truncated".
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take_u64(&mut self) -> Result<u64, JournalError> {
        if self.bytes.len() < self.at + 8 {
            return Err(JournalError::Corrupt);
        }
        let value = u64::from_le_bytes(
            self.bytes[self.at..self.at + 8]
                .try_into()
                .expect("len checked"),
        );
        self.at += 8;
        Ok(value)
    }
}

fn put_price(out: &mut Vec<u8>, price: Price) {
    put_u64(out, price.tick());
}

fn take_price(cursor: &mut Cursor<'_>) -> Result<Price, JournalError> {
    Price::from_ticks(cursor.take_u64()?).map_err(|_| JournalError::Corrupt)
}

fn put_qty(out: &mut Vec<u8>, qty: Qty) {
    put_u64(out, qty.lot());
}

fn take_qty(cursor: &mut Cursor<'_>) -> Result<Qty, JournalError> {
    Qty::from_lots(cursor.take_u64()?).map_err(|_| JournalError::Corrupt)
}

fn encode_entry(entry: &Entry) -> Vec<u8> {
    let mut out = Vec::new();
    put_u64(&mut out, u64::from(entry.accepted));
    match entry.command {
        Command::Deposit {
            user,
            currency,
            amount,
        } => {
            put_u64(&mut out, TAG_DEPOSIT);
            put_u64(&mut out, user.0);
            put_u64(&mut out, currency.0);
            put_u64(&mut out, amount);
        }
        Command::Withdraw {
            user,
            currency,
            amount,
        } => {
            put_u64(&mut out, TAG_WITHDRAW);
            put_u64(&mut out, user.0);
            put_u64(&mut out, currency.0);
            put_u64(&mut out, amount);
        }
        Command::SetPositionLimit {
            user,
            currency,
            cap,
        } => {
            put_u64(&mut out, TAG_SET_LIMIT);
            put_u64(&mut out, user.0);
            put_u64(&mut out, currency.0);
            put_u64(&mut out, cap);
        }
        Command::ClearPositionLimit { user, currency } => {
            put_u64(&mut out, TAG_CLEAR_LIMIT);
            put_u64(&mut out, user.0);
            put_u64(&mut out, currency.0);
        }
        Command::Place { order, reserve } => {
            put_u64(&mut out, TAG_PLACE);
            put_u64(&mut out, order.id.0);
            put_u64(&mut out, order.user.0);
            put_u64(&mut out, order.symbol.0);
            put_u64(
                &mut out,
                match order.side {
                    Side::Bid => 0,
                    Side::Ask => 1,
                },
            );
            // Type: limit = tag 0 + price ticks; market = tag 1.
            match order.order_type {
                OrderType::Limit { price } => {
                    put_u64(&mut out, 0);
                    put_price(&mut out, price);
                }
                OrderType::Market => put_u64(&mut out, 1),
            }
            // TIF: GTC/IOC/FOK = 0/1/2.
            put_u64(
                &mut out,
                match order.tif {
                    TimeInForce::Gtc => 0,
                    TimeInForce::Ioc => 1,
                    TimeInForce::Fok => 2,
                },
            );
            put_qty(&mut out, order.quantity);
            put_u64(&mut out, order.timestamp);
            match reserve {
                Some(price) => {
                    put_u64(&mut out, 1);
                    put_price(&mut out, price);
                }
                None => put_u64(&mut out, 0),
            }
        }
        Command::Cancel { id } => {
            put_u64(&mut out, TAG_CANCEL);
            put_u64(&mut out, id.0);
        }
        Command::Reduce { id, by } => {
            put_u64(&mut out, TAG_REDUCE);
            put_u64(&mut out, id.0);
            put_qty(&mut out, by);
        }
        Command::Move { id, new_price } => {
            put_u64(&mut out, TAG_MOVE);
            put_u64(&mut out, id.0);
            put_price(&mut out, new_price);
        }
    }
    out
}

fn decode_entry(payload: &[u8]) -> Result<Entry, JournalError> {
    let mut cursor = Cursor {
        bytes: payload,
        at: 0,
    };
    let accepted = cursor.take_u64()?;
    if accepted > 1 {
        return Err(JournalError::Corrupt);
    }
    let accepted = accepted == 1;
    let tag = cursor.take_u64()?;
    let command = match tag {
        TAG_DEPOSIT => Command::Deposit {
            user: UserId(cursor.take_u64()?),
            currency: CurrencyId(cursor.take_u64()?),
            amount: cursor.take_u64()?,
        },
        TAG_WITHDRAW => Command::Withdraw {
            user: UserId(cursor.take_u64()?),
            currency: CurrencyId(cursor.take_u64()?),
            amount: cursor.take_u64()?,
        },
        TAG_SET_LIMIT => Command::SetPositionLimit {
            user: UserId(cursor.take_u64()?),
            currency: CurrencyId(cursor.take_u64()?),
            cap: cursor.take_u64()?,
        },
        TAG_CLEAR_LIMIT => Command::ClearPositionLimit {
            user: UserId(cursor.take_u64()?),
            currency: CurrencyId(cursor.take_u64()?),
        },
        TAG_PLACE => {
            let id = OrderId(cursor.take_u64()?);
            let user = UserId(cursor.take_u64()?);
            let symbol = SymbolId(cursor.take_u64()?);
            let side = match cursor.take_u64()? {
                0 => Side::Bid,
                1 => Side::Ask,
                _ => return Err(JournalError::Corrupt),
            };
            let order_type = match cursor.take_u64()? {
                0 => OrderType::Limit {
                    price: take_price(&mut cursor)?,
                },
                1 => OrderType::Market,
                _ => return Err(JournalError::Corrupt),
            };
            let tif = match cursor.take_u64()? {
                0 => TimeInForce::Gtc,
                1 => TimeInForce::Ioc,
                2 => TimeInForce::Fok,
                _ => return Err(JournalError::Corrupt),
            };
            let quantity = take_qty(&mut cursor)?;
            let timestamp = cursor.take_u64()?;
            let reserve = match cursor.take_u64()? {
                0 => None,
                1 => Some(take_price(&mut cursor)?),
                _ => return Err(JournalError::Corrupt),
            };
            // Order::new re-validates type/TIF pairing at decode — a corrupt
            // pairing is Corrupt, never a constructed nonsense order.
            let order = Order::new(id, user, symbol, side, order_type, tif, quantity, timestamp)
                .map_err(|_| JournalError::Corrupt)?;
            Command::Place { order, reserve }
        }
        TAG_CANCEL => Command::Cancel {
            id: OrderId(cursor.take_u64()?),
        },
        TAG_REDUCE => Command::Reduce {
            id: OrderId(cursor.take_u64()?),
            by: take_qty(&mut cursor)?,
        },
        TAG_MOVE => Command::Move {
            id: OrderId(cursor.take_u64()?),
            new_price: take_price(&mut cursor)?,
        },
        _ => return Err(JournalError::Corrupt),
    };
    // The payload must end exactly here — a short trailing field is the
    // same corruption class as a short frame.
    if cursor.at != cursor.bytes.len() {
        return Err(JournalError::Corrupt);
    }
    Ok(Entry { command, accepted })
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
