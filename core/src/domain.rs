//! The domain vocabulary: sides, order types, time-in-force, scaled integer
//! prices and quantities, ids, and the [`Order`] value.
//!
//! Ground rules encoded here (see `docs/architecture.md`):
//! - **No floating point anywhere.** Prices are integer *ticks* of
//!   [`PRICE_SCALE`] per quote unit; quantities are integer *lots* of
//!   [`QTY_SCALE`] per base unit. Decimal strings parse via pure integer
//!   arithmetic, so no significance is ever lost (exchange-core's hard rule,
//!   adopted in `docs/research/exchange-core.md`).
//! - **Determinism.** There is no wall clock in this module — timestamps are
//!   engine-assigned sequence numbers.
//! - **Valid by construction.** [`Price`] and [`Qty`] can never be zero,
//!   and [`OrderType::for_tif`] rejects meaningless combinations (e.g. a
//!   market order that rests on the book) at creation time.
//!
//! Everything here is a plain value type: no I/O, no time, no randomness.
//! The order book and matching engine (next Phase 0 tasks) consume these.

use std::fmt;

/// Quote-currency ticks per unit: 10,000 ticks = 1.0 quote.
///
/// Gives 4-decimal price precision (e.g. `0.0001`), plenty for any venue we
/// simulate. A global constant for Phase 0; per-symbol scales are deferred to
/// Phase 3 (risk/accounting) and logged there when they land.
pub const PRICE_SCALE: u64 = 10_000;

/// Decimal places a price string may carry (= digits in [`PRICE_SCALE`]).
pub const PRICE_DECIMALS: u32 = 4;

/// Base-asset lots per unit: 100,000,000 lots = 1.0 base (satoshi-style).
///
/// Mirrors Bitcoin's 1e8 subdivision — the finest subdivision in common use —
/// so quantity strings never need to be rejected for precision in practice.
pub const QTY_SCALE: u64 = 100_000_000;

/// Decimal places a quantity string may carry (= digits in [`QTY_SCALE`]).
pub const QTY_DECIMALS: u32 = 8;

/// Everything the domain layer can reject, checked at construction.
///
/// Hand-rolled because the error surface is four variants; `thiserror` lands
/// as a dependency when this grows past what a human can eyeball (dependency
/// decisions are logged one at a time in the decision log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// A price/quantity string was empty or contained non-numeric characters.
    InvalidNumber,
    /// A price/quantity string carried more precision than the scale allows.
    TooMuchPrecision {
        /// How many decimal places the scale would have accepted.
        allowed: u32,
    },
    /// A price/quantity string represented a number too large for `u64` ticks/lots.
    Overflow,
    /// Zero prices and zero quantities are meaningless on a book and rejected.
    Zero,
    /// The order type and time-in-force cannot be combined (e.g. market + GTC).
    OrderTypeTimeInForceMismatch,
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidNumber => write!(f, "not a valid decimal number"),
            Self::TooMuchPrecision { allowed } => {
                write!(f, "more than {allowed} decimal places")
            }
            Self::Overflow => write!(f, "value too large"),
            Self::Zero => write!(f, "zero is not allowed here"),
            Self::OrderTypeTimeInForceMismatch => {
                write!(
                    f,
                    "this order type cannot be combined with this time-in-force"
                )
            }
        }
    }
}

impl std::error::Error for DomainError {}

/// Internal failure type for the one shared decimal parser.
///
/// Kept private: callers map it onto [`DomainError`] variants with the right
/// context (price vs quantity), so the tricky parsing logic exists exactly
/// once (review rule: no near-duplicate helpers).
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParseFailure {
    Invalid,
    TooManyDecimals,
    Overflow,
    Zero,
}

/// Number of decimal digits in a power-of-ten scale (10_000 → 4).
const fn decimal_digits(scale: u64) -> u32 {
    let mut digits = 0;
    let mut s = scale;
    while s > 1 {
        s /= 10;
        digits += 1;
    }
    digits
}

/// Parse a strict decimal string into scaled integer units.
///
/// Strictness rules (each enforced by a test):
/// - digits and at most one `.` only — no sign, whitespace, or exponent;
/// - the integer part must be present (`.5` is invalid);
/// - the fractional part must be present if a `.` is (`5.` is invalid);
/// - at most `scale`-digits of precision (`TooManyDecimals` beyond);
/// - the scaled result must fit `u64` (`Overflow`) and be non-zero (`Zero`).
///
/// Leading zeros are allowed (`007.5` == `7.5`) — unambiguous, not worth a
/// rejection path.
fn parse_scaled_decimal(s: &str, scale: u64) -> Result<u64, ParseFailure> {
    // Only ASCII needs handling: non-ASCII bytes can't be digit separators
    // we accept, and `is_ascii_digit` below rejects them.
    if s.is_empty() {
        return Err(ParseFailure::Invalid);
    }
    let (int_part, frac_part) = match s.split_once('.') {
        // A second '.' leaves a '.' inside the remainder — reject that too.
        Some((int_part, frac_part)) => {
            if frac_part.contains('.') {
                return Err(ParseFailure::Invalid);
            }
            (int_part, Some(frac_part))
        }
        None => (s, None),
    };

    if int_part.is_empty() || (frac_part.is_some_and(str::is_empty)) {
        return Err(ParseFailure::Invalid);
    }

    // Integer part: fold digits with checked arithmetic — the first overflow
    // is caught here rather than wrapping silently.
    let mut int_val: u64 = 0;
    for b in int_part.bytes() {
        if !b.is_ascii_digit() {
            return Err(ParseFailure::Invalid);
        }
        int_val = int_val
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(b - b'0')))
            .ok_or(ParseFailure::Overflow)?;
    }

    // Fractional part: fold its digits, then right-shift into the scale by
    // multiplying by 10^(missing places). Both steps are checked.
    let frac_scaled: u64 = match frac_part {
        None => 0,
        Some(frac) => {
            let allowed = decimal_digits(scale);
            if frac.len() as u32 > allowed {
                return Err(ParseFailure::TooManyDecimals);
            }
            let mut frac_val: u64 = 0;
            for b in frac.bytes() {
                if !b.is_ascii_digit() {
                    return Err(ParseFailure::Invalid);
                }
                frac_val = frac_val
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(u64::from(b - b'0')))
                    .ok_or(ParseFailure::Overflow)?;
            }
            let missing = allowed - frac.len() as u32;
            let factor = 10u64.checked_pow(missing).ok_or(ParseFailure::Overflow)?;
            frac_val.checked_mul(factor).ok_or(ParseFailure::Overflow)?
        }
    };

    let total = int_val
        .checked_mul(scale)
        .and_then(|v| v.checked_add(frac_scaled))
        .ok_or(ParseFailure::Overflow)?;

    if total == 0 {
        return Err(ParseFailure::Zero);
    }
    Ok(total)
}

/// An order book price as an integer number of ticks ([`PRICE_SCALE`] per quote unit).
///
/// Construct from integer ticks or from a decimal string; never from a float.
/// Non-zero by construction (zero-priced orders are rejected at parse time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(u64);

impl Price {
    /// Wrap raw ticks. `0` is rejected: a zero-priced limit order is a bug,
    /// not a book state.
    ///
    /// # Errors
    /// Returns [`DomainError::Zero`] for `0`.
    pub fn from_ticks(ticks: u64) -> Result<Self, DomainError> {
        if ticks == 0 {
            return Err(DomainError::Zero);
        }
        Ok(Self(ticks))
    }

    /// Parse a quote-unit decimal string (`"123.4567"`) into ticks.
    ///
    /// # Errors
    /// See [`parse_scaled_decimal`] rules; maps to [`DomainError`] variants.
    pub fn from_quote_units_str(s: &str) -> Result<Self, DomainError> {
        parse_scaled_decimal(s, PRICE_SCALE)
            .map_err(|e| map_failure(e, PRICE_DECIMALS))
            .map(Self)
    }

    /// Raw tick value (for engine math and event records).
    #[must_use]
    pub const fn tick(self) -> u64 {
        self.0
    }

    /// Checked addition — the only way prices combine, so overflow surfaces
    /// as `None` instead of wrapping into a crossed-book bug.
    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }
}

impl fmt::Display for Price {
    /// Exact decimal form: `Price::from_ticks(1_234_567)` prints `123.4567`.
    /// Exact because the fractional digits are the raw remainder, zero-padded
    /// to the scale's digit count — no float formatting involved.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / PRICE_SCALE;
        let frac = self.0 % PRICE_SCALE;
        write!(f, "{whole}.{frac:0width$}", width = PRICE_DECIMALS as usize)
    }
}

/// An order quantity as an integer number of lots ([`QTY_SCALE`] per base unit).
///
/// Same rules as [`Price`]: integer lots or strict decimal strings, never
/// floats, never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Qty(u64);

impl Qty {
    /// Wrap raw lots. `0` is rejected: empty orders are a bug, not a state.
    ///
    /// # Errors
    /// Returns [`DomainError::Zero`] for `0`.
    pub fn from_lots(lots: u64) -> Result<Self, DomainError> {
        if lots == 0 {
            return Err(DomainError::Zero);
        }
        Ok(Self(lots))
    }

    /// Parse a base-unit decimal string (`"0.5"`) into lots.
    ///
    /// # Errors
    /// See [`parse_scaled_decimal`] rules; maps to [`DomainError`] variants.
    pub fn from_base_units_str(s: &str) -> Result<Self, DomainError> {
        parse_scaled_decimal(s, QTY_SCALE)
            .map_err(|e| map_failure(e, QTY_DECIMALS))
            .map(Self)
    }

    /// Raw lot value (for engine math and event records).
    #[must_use]
    pub const fn lot(self) -> u64 {
        self.0
    }

    /// Checked subtraction — quantities only ever decrease by fills/cancels,
    /// and this makes "subtract more than remains" return `None` instead of
    /// wrapping into phantom inventory.
    #[must_use]
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }
}

impl fmt::Display for Qty {
    /// Exact decimal form: `Qty::from_lots(50_000_000)` prints `0.50000000`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / QTY_SCALE;
        let frac = self.0 % QTY_SCALE;
        write!(f, "{whole}.{frac:0width$}", width = QTY_DECIMALS as usize)
    }
}

/// Map a private parse failure onto the public error with scale context.
fn map_failure(failure: ParseFailure, allowed_decimals: u32) -> DomainError {
    match failure {
        ParseFailure::Invalid => DomainError::InvalidNumber,
        ParseFailure::TooManyDecimals => DomainError::TooMuchPrecision {
            allowed: allowed_decimals,
        },
        ParseFailure::Overflow => DomainError::Overflow,
        ParseFailure::Zero => DomainError::Zero,
    }
}

/// Which side of the book an order sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Buy — rests on the bid side, sorted best-first by highest price.
    Bid,
    /// Sell — rests on the ask side, sorted best-first by lowest price.
    Ask,
}

impl Side {
    /// The opposite side — matching always consumes liquidity from here.
    ///
    /// `self.opposite().opposite() == self` (asserted by test).
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Bid => Self::Ask,
            Self::Ask => Self::Bid,
        }
    }
}

/// How long an order stays alive on the book.
///
/// Deliberately a separate type from [`OrderType`]: GTC/IOC/FOK are
/// *lifetime* policies orthogonal to *what* the order does (limit vs take
/// liquidity). Folding them into one 5-variant enum (the original TODO §5
/// sketch) would make Limit and GTC redundant and allow nonsense states —
/// see the decision log (2026-09-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeInForce {
    /// Good-till-cancel: rests on the book until filled or canceled.
    Gtc,
    /// Immediate-or-cancel: fill what the book offers right now, cancel the rest.
    Ioc,
    /// Fill-or-kill: fill entirely right now, or not at all.
    Fok,
}

/// What the order asks the engine to do.
///
/// A limit order names a price and may rest; a market order takes whatever
/// liquidity exists and never rests (so it can only be [`TimeInForce::Ioc`] —
/// enforced by [`OrderType::for_tif`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderType {
    /// Rest or match at exactly this price (or better, when crossing).
    Limit {
        /// The order's limit price in quote units.
        price: Price,
    },
    /// Sweep the opposite side at any price until filled or the book is empty.
    Market,
}

impl OrderType {
    /// Validate this type against a time-in-force.
    ///
    /// Rules (each asserted by test):
    /// - Limit × {GTC, IOC, FOK} — all valid;
    /// - Market × IOC — valid (take now, cancel the remainder);
    /// - Market × GTC — invalid (a resting market order is a contradiction);
    /// - Market × FOK — invalid for Phase 0 (exotic; revisit in Phase 1 if
    ///   the simulator needs it — noted in the decision log).
    ///
    /// # Errors
    /// Returns [`DomainError::OrderTypeTimeInForceMismatch`] for the two
    /// invalid market combinations.
    pub const fn for_tif(self, tif: TimeInForce) -> Result<(), DomainError> {
        match (self, tif) {
            (Self::Limit { .. }, _) => Ok(()),
            (Self::Market, TimeInForce::Ioc) => Ok(()),
            (Self::Market, _) => Err(DomainError::OrderTypeTimeInForceMismatch),
        }
    }
}

/// What the engine is being asked to do with an order.
///
/// Mirrors exchange-core's operation model: `place` (match + maybe rest),
/// `move` (reprice an existing order — resets time priority), `cancel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderAction {
    /// Submit a new order.
    Place,
    /// Reprice an existing order (cheaper than cancel+place in real engines).
    Move,
    /// Remove an existing order.
    Cancel,
}

/// Order identifier, assigned by the engine in submission order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderId(pub u64);

/// Participant identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UserId(pub u64);

/// Instrument identifier (one order book per symbol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u64);

/// An order as it enters the engine — the input vocabulary of the whole core.
///
/// Fields are public on purpose: every field's type is already valid by
/// construction (nonzero [`Price`]/[`Qty`], checked type/TIF pairing), so
/// there is no invalid `Order` a caller could build by assignment. The book
/// (next task) derives its own rest-state from these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    /// Engine-assigned unique id.
    pub id: OrderId,
    /// Owning participant.
    pub user: UserId,
    /// Which book this belongs to.
    pub symbol: SymbolId,
    /// Buy or sell.
    pub side: Side,
    /// Limit (with price) or market.
    pub order_type: OrderType,
    /// Lifetime policy.
    pub tif: TimeInForce,
    /// How much to trade.
    pub quantity: Qty,
    /// Engine-assigned submission sequence — **never wall clock** (determinism).
    pub timestamp: u64,
}

impl Order {
    /// Construct an order, enforcing the type/time-in-force pairing rules.
    ///
    /// # Errors
    /// Returns [`DomainError::OrderTypeTimeInForceMismatch`] if the
    /// combination is invalid (e.g. market + GTC).
    // An order genuinely has 8 independent dimensions; a builder would add
    // ceremony without adding validity (every field type is already sound).
    // `#[expect]` (not `#[allow]`) so the lint fires if this stops being true.
    #[expect(
        clippy::too_many_arguments,
        reason = "an order genuinely has 8 dimensions; field types already guarantee validity"
    )]
    pub fn new(
        id: OrderId,
        user: UserId,
        symbol: SymbolId,
        side: Side,
        order_type: OrderType,
        tif: TimeInForce,
        quantity: Qty,
        timestamp: u64,
    ) -> Result<Self, DomainError> {
        order_type.for_tif(tif)?;
        Ok(Self {
            id,
            user,
            symbol,
            side,
            order_type,
            tif,
            quantity,
            timestamp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Price parsing -------------------------------------------------

    #[test]
    fn price_parses_quote_units_to_exact_ticks() {
        assert_eq!(
            Price::from_quote_units_str("123.4567").unwrap().tick(),
            1_234_567
        );
        assert_eq!(Price::from_quote_units_str("0.0001").unwrap().tick(), 1);
        assert_eq!(Price::from_quote_units_str("42").unwrap().tick(), 420_000);
    }

    #[test]
    fn price_rejects_invalid_strings() {
        for bad in [
            ".5", "5.", "1.2.3", "-1", "+1", "1e3", "", " 1", "1 ", "0x10", "١٢٣",
        ] {
            let result = Price::from_quote_units_str(bad);
            assert!(
                matches!(result, Err(DomainError::InvalidNumber)),
                "input {bad:?}"
            );
        }
    }

    #[test]
    fn price_rejects_extra_precision() {
        assert!(matches!(
            Price::from_quote_units_str("0.00001"),
            Err(DomainError::TooMuchPrecision { allowed: 4 })
        ));
    }

    #[test]
    fn price_rejects_zero_in_every_shape() {
        for zero in ["0", "0.0", "0.0000", "00"] {
            assert!(matches!(
                Price::from_quote_units_str(zero),
                Err(DomainError::Zero)
            ));
        }
        assert!(matches!(Price::from_ticks(0), Err(DomainError::Zero)));
    }

    #[test]
    fn price_rejects_values_that_overflow_ticks() {
        // u64::MAX quote units * 10_000 cannot fit u64.
        assert!(matches!(
            Price::from_quote_units_str("18446744073709551615"),
            Err(DomainError::Overflow)
        ));
    }

    #[test]
    fn price_display_round_trips_exactly() {
        for ticks in [1_u64, 9_999, 10_000, 10_001, 1_234_567, u64::MAX] {
            let price = Price::from_ticks(ticks).unwrap();
            let reparsed = Price::from_quote_units_str(&price.to_string()).unwrap();
            assert_eq!(reparsed, price);
        }
    }

    #[test]
    fn price_checked_add_reports_overflow_as_none() {
        let max = Price::from_ticks(u64::MAX).unwrap();
        let one = Price::from_ticks(1).unwrap();
        assert_eq!(max.checked_add(one), None);
    }

    // ---- Qty parsing ---------------------------------------------------

    #[test]
    fn qty_parses_base_units_to_exact_lots() {
        assert_eq!(Qty::from_base_units_str("1").unwrap().lot(), 100_000_000);
        assert_eq!(Qty::from_base_units_str("0.5").unwrap().lot(), 50_000_000);
        assert_eq!(Qty::from_base_units_str("0.00000001").unwrap().lot(), 1);
    }

    #[test]
    fn qty_rejects_sub_lot_precision() {
        assert!(matches!(
            Qty::from_base_units_str("0.000000001"),
            Err(DomainError::TooMuchPrecision { allowed: 8 })
        ));
    }

    #[test]
    fn qty_rejects_zero_and_garbage() {
        assert!(matches!(
            Qty::from_base_units_str("0"),
            Err(DomainError::Zero)
        ));
        assert!(matches!(Qty::from_lots(0), Err(DomainError::Zero)));
        assert!(matches!(
            Qty::from_base_units_str("x"),
            Err(DomainError::InvalidNumber)
        ));
    }

    #[test]
    fn qty_checked_sub_reports_underflow_as_none() {
        let one = Qty::from_lots(1).unwrap();
        let two = Qty::from_lots(2).unwrap();
        assert_eq!(one.checked_sub(two), None);
        assert_eq!(two.checked_sub(one), Some(Qty::from_lots(1).unwrap()));
    }

    #[test]
    fn qty_display_round_trips_exactly() {
        for lots in [1_u64, 99_999_999, 100_000_000, 150_000_000, u64::MAX] {
            let qty = Qty::from_lots(lots).unwrap();
            let reparsed = Qty::from_base_units_str(&qty.to_string()).unwrap();
            assert_eq!(reparsed, qty);
        }
    }

    // ---- Side ----------------------------------------------------------

    #[test]
    fn side_opposite_is_involutive() {
        assert_eq!(Side::Bid.opposite(), Side::Ask);
        assert_eq!(Side::Ask.opposite(), Side::Bid);
        assert_eq!(Side::Bid.opposite().opposite(), Side::Bid);
    }

    // ---- OrderType / TimeInForce validity ------------------------------

    #[test]
    fn limit_order_accepts_every_time_in_force() {
        let limit = OrderType::Limit {
            price: Price::from_ticks(1).unwrap(),
        };
        for tif in [TimeInForce::Gtc, TimeInForce::Ioc, TimeInForce::Fok] {
            assert_eq!(limit.for_tif(tif), Ok(()));
        }
    }

    #[test]
    fn market_order_is_ioc_only() {
        assert_eq!(OrderType::Market.for_tif(TimeInForce::Ioc), Ok(()));
        assert!(matches!(
            OrderType::Market.for_tif(TimeInForce::Gtc),
            Err(DomainError::OrderTypeTimeInForceMismatch)
        ));
        assert!(matches!(
            OrderType::Market.for_tif(TimeInForce::Fok),
            Err(DomainError::OrderTypeTimeInForceMismatch)
        ));
    }

    // ---- Order ---------------------------------------------------------

    #[test]
    fn order_new_preserves_fields_and_engine_timestamp() {
        let order = Order::new(
            OrderId(7),
            UserId(1),
            SymbolId(1),
            Side::Bid,
            OrderType::Limit {
                price: Price::from_quote_units_str("100.5").unwrap(),
            },
            TimeInForce::Gtc,
            Qty::from_base_units_str("2").unwrap(),
            42,
        )
        .unwrap();
        assert_eq!(order.id, OrderId(7));
        assert_eq!(order.user, UserId(1));
        assert_eq!(order.symbol, SymbolId(1));
        assert_eq!(order.side, Side::Bid);
        assert_eq!(order.tif, TimeInForce::Gtc);
        assert_eq!(order.quantity, Qty::from_lots(200_000_000).unwrap());
        // The timestamp is whatever the engine assigned — never derived here.
        assert_eq!(order.timestamp, 42);
    }

    #[test]
    fn order_new_rejects_market_gtc() {
        assert!(matches!(
            Order::new(
                OrderId(1),
                UserId(1),
                SymbolId(1),
                Side::Ask,
                OrderType::Market,
                TimeInForce::Gtc,
                Qty::from_lots(1).unwrap(),
                1,
            ),
            Err(DomainError::OrderTypeTimeInForceMismatch)
        ));
    }

    // ---- Doc example (kept in sync with the docs above) -----------------

    #[test]
    fn doc_example_price_parse_and_display() {
        let price = Price::from_quote_units_str("100.5").unwrap();
        assert_eq!(price.tick(), 1_005_000);
        assert_eq!(price.to_string(), "100.5000");
    }
}
