// Rust guideline compliant 2026-03-30

//! Numbers that survive the log.
//!
//! The event log is append-only and is the only source of truth, so a number
//! written into it is permanent. Two things go wrong with a number that is
//! not finite:
//!
//! - `serde_json` writes it as `null`, and `null` is not a valid `f32`, so
//!   the event that carried it can never be read back.
//! - SQLite cannot store a NaN at all: it stores it as `NULL`, which fails
//!   the `NOT NULL` constraint on the salience column and silently drops the
//!   affect state from the row.
//!
//! So every number is repaired to a finite value on the way in, and a
//! malformed number is repaired to a finite default on the way out. A finite
//! number is never changed, so a log written today holds exactly the bytes
//! it always held.
//!
//! Every repair is reported. A repair turns two different facts — a number
//! that was not a number, and a number that was never written — into the
//! same finite value, so a log that quietly held either of them would be
//! claiming a number the mind never had.

use std::cell::RefCell;
use std::fmt;

use serde::de;
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serializer};

/// The value a number that is not finite is repaired to.
///
/// A number that is not finite carries no information — no magnitude, no
/// direction — so the log keeps the neutral zero. Zero is what `si-affect`
/// gives an axis that is not a number, and what the read path gives a
/// `null`, so a repaired number and a number that was never there look the
/// same in the log.
pub(crate) const NON_FINITE_DEFAULT: f32 = 0.0;

/// The number of axes in an affect vector.
///
/// The affect state is three axes wide (`valence`, `arousal`, `tension`), so
/// a vector written into an event is three numbers long. A vector of another
/// length is a malformed event, not a repaired one.
const VECTOR_LEN: usize = 3;

/// What a repair reports when it has no row to name.
///
/// A payload can be read outside a row read — the crate's own tests do it —
/// and a report with no row must still say so rather than leave the field
/// out.
const NO_ROW: &str = "-";

/// What a repair reports when the field has no axis.
///
/// A field that carries a single number has no axis; a field that carries a
/// vector has one of three.
const NO_AXIS: &str = "-";

/// The axis a repair was in, as it is reported.
///
/// The vector is three axes wide, so the axis is one of three numbers.
pub(crate) fn axis_marker(axis: Option<usize>) -> String {
    match axis {
        Some(axis) => axis.to_string(),
        None => NO_AXIS.to_string(),
    }
}

/// What a number was before it was repaired.
///
/// The repair turns all of these into the same finite value, so the report
/// has to carry which one it was: `+Inf` and `-Inf` are different facts
/// about the mind, and a `null` says the payload had lost the number
/// altogether.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Original {
    /// The payload held a `null`: the number was never written.
    Null,
    /// The number was not a number.
    NotANumber,
    /// The number was an infinity.
    PositiveInfinity,
    /// The number was an infinity.
    NegativeInfinity,
    /// The column held `NULL`: the axis was never written.
    Missing,
}

impl Original {
    /// What a number is, when it is not finite.
    ///
    /// A finite number has no repair to report, so it has no `Original`.
    pub(crate) fn of(value: f32) -> Option<Original> {
        if value.is_nan() {
            Some(Original::NotANumber)
        } else if value == f32::INFINITY {
            Some(Original::PositiveInfinity)
        } else if value == f32::NEG_INFINITY {
            Some(Original::NegativeInfinity)
        } else {
            None
        }
    }
}

impl fmt::Display for Original {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Original::Null => "null",
            Original::NotANumber => "not a number",
            Original::PositiveInfinity => "positive infinity",
            Original::NegativeInfinity => "negative infinity",
            Original::Missing => "missing",
        };
        f.write_str(text)
    }
}

/// One number that was repaired.
///
/// A repair names the field it happened in, so a report can say which number
/// of which event it was talking about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Repair {
    /// The field the number was in.
    pub field: &'static str,
    /// Which axis of the vector, when the field carries a vector.
    pub axis: Option<usize>,
    /// What the number was before it was repaired.
    pub original: Original,
}

thread_local! {
    /// The repairs made while one payload is being read.
    ///
    /// `None` means no payload is being read, so a repair has no row to name
    /// and reports itself.
    static REPAIRS: RefCell<Option<Vec<Repair>>> = const { RefCell::new(None) };
}

/// Reads one payload, collecting every repair it makes.
///
/// The repairs are handed back to the caller, which is the only place that
/// knows which row is being read.
pub(crate) fn reading_payload<T>(read: impl FnOnce() -> T) -> (T, Vec<Repair>) {
    let previous = REPAIRS.with(|repairs| repairs.replace(Some(Vec::new())));
    let out = read();
    let collected = REPAIRS.with(|repairs| match &mut *repairs.borrow_mut() {
        Some(repairs) => std::mem::take(repairs),
        None => Vec::new(),
    });
    REPAIRS.with(|repairs| *repairs.borrow_mut() = previous);
    (out, collected)
}

/// Records one repair made while a payload is being read.
///
/// A repair made outside a payload read has no row to name, so it reports
/// itself rather than being dropped.
fn record(field: &'static str, axis: Option<usize>, original: Original) {
    let repair = Repair {
        field,
        axis,
        original,
    };
    let recorded = REPAIRS.with(|repairs| match repairs.borrow_mut().as_mut() {
        Some(repairs) => {
            repairs.push(repair);
            true
        }
        None => false,
    });
    if !recorded {
        report_payload_repair(None, &repair);
    }
}

/// Reports a number that was repaired while a payload was being read.
///
/// `row` is the id of the row the payload belongs to, when there is one.
pub(crate) fn report_payload_repair(row: Option<&str>, repair: &Repair) {
    tracing::warn!(
        name: "log.payload.repaired",
        row = %row.unwrap_or(NO_ROW),
        field = repair.field,
        axis = %axis_marker(repair.axis),
        original = %repair.original,
        "a number in the payload was not a number and has been repaired"
    );
}

/// Repairs one number so it can be written to the log.
///
/// A finite number is returned unchanged, so a log written today holds the
/// same bytes it always held. A number that is not finite becomes
/// [`NON_FINITE_DEFAULT`].
pub(crate) fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        NON_FINITE_DEFAULT
    }
}

/// Repairs one affect vector and records what it had to repair.
///
/// The repairs are pushed onto `repairs`, so the caller can report them with
/// the row they belong to.
pub(crate) fn repaired_vector(
    field: &'static str,
    values: [f32; 3],
    repairs: &mut Vec<Repair>,
) -> [f32; 3] {
    let mut repaired = values;
    for (index, axis) in values.iter().enumerate() {
        if let Some(original) = Original::of(*axis) {
            repairs.push(Repair {
                field,
                axis: Some(index),
                original,
            });
            repaired[index] = finite(*axis);
        }
    }
    repaired
}

/// Writes one number, repaired.
///
/// The number that reaches the log is always finite, whichever way the event
/// was built.
pub(crate) fn serialize_finite<S>(value: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_f32(finite(*value))
}

/// Writes an affect vector, repaired.
///
/// The vector is written as a tuple, which is what serde's own array
/// serialisation writes. A length-prefixed binary sink reads a tuple and a
/// sequence differently, so the shape the log holds must not depend on the
/// format it happens to be written in.
pub(crate) fn serialize_finite_vector<S>(
    values: &[f32; VECTOR_LEN],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut tuple = serializer.serialize_tuple(VECTOR_LEN)?;
    for axis in values {
        tuple.serialize_element(&finite(*axis))?;
    }
    tuple.end()
}

/// Reads one number, repaired, and says which field it was in.
///
/// `serde_json` writes a number that is not finite as `null`, and `null` is
/// not a valid `f32`, so the value is read as an optional number: a `null`
/// becomes the neutral default instead of failing the whole row. A number
/// that overflows `f32` is read as an infinity and repaired the same way.
///
/// A missing field is still an error. Only a malformed number is repaired
/// here; an absent one is absent data, and inventing it would put a number
/// into the log that nobody wrote.
pub(crate) fn deserialize_finite_as<'de, D>(
    deserializer: D,
    field: &'static str,
) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<f32>::deserialize(deserializer)? {
        Some(value) => match Original::of(value) {
            Some(original) => {
                record(field, None, original);
                Ok(NON_FINITE_DEFAULT)
            }
            None => Ok(value),
        },
        None => {
            record(field, None, Original::Null);
            Ok(NON_FINITE_DEFAULT)
        }
    }
}

/// Reads an affect vector, repaired, and says which field it was in.
///
/// The vector is read as a list of optional numbers, so an axis that was
/// written as `null` becomes the neutral default instead of failing the
/// whole row.
///
/// A vector of the wrong length is still an error: a missing axis is missing
/// data, not a malformed number.
pub(crate) fn deserialize_finite_vector_as<'de, D>(
    deserializer: D,
    field: &'static str,
) -> Result<[f32; 3], D::Error>
where
    D: Deserializer<'de>,
{
    let axes = Vec::<Option<f32>>::deserialize(deserializer)?;
    if axes.len() != VECTOR_LEN {
        return Err(de::Error::invalid_length(
            axes.len(),
            &"an array of three numbers",
        ));
    }
    let mut repaired = [NON_FINITE_DEFAULT; VECTOR_LEN];
    for (index, axis) in axes.iter().enumerate() {
        repaired[index] = match axis {
            Some(value) => match Original::of(*value) {
                Some(original) => {
                    record(field, Some(index), original);
                    NON_FINITE_DEFAULT
                }
                None => *value,
            },
            None => {
                record(field, Some(index), Original::Null);
                NON_FINITE_DEFAULT
            }
        };
    }
    Ok(repaired)
}

/// The deserialisers that name the field they read.
///
/// serde hands a deserialiser the value and nothing else, so the field name
/// has to come from the function that reads it. A repair that cannot say
/// which field it was in is a repair that cannot be acted on.
macro_rules! scalar_deserialisers {
    ($($name:ident => $field:literal),* $(,)?) => {
        $(
            /// Reads one number, repaired, and says which field it was in.
            pub(crate) fn $name<'de, D>(deserializer: D) -> Result<f32, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserialize_finite_as(deserializer, $field)
            }
        )*
    };
}

scalar_deserialisers!(
    deserialize_confidence => "confidence",
    deserialize_valence => "valence",
    deserialize_arousal => "arousal",
    deserialize_tension => "tension",
    deserialize_salience => "salience",
);

/// Reads the `delta` of a felt event, repaired, and says which field it was in.
pub(crate) fn deserialize_delta<'de, D>(deserializer: D) -> Result<[f32; 3], D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_finite_vector_as(deserializer, "delta")
}

/// Reads the `after` of a felt event, repaired, and says which field it was in.
pub(crate) fn deserialize_after<'de, D>(deserializer: D) -> Result<[f32; 3], D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_finite_vector_as(deserializer, "after")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fmt;

    use serde::ser::{self, SerializeTuple};

    /// A serializer that accepts a tuple of numbers and nothing else.
    ///
    /// The bytes `serde_json` writes are the same for a tuple and a sequence,
    /// so the only way to see which shape was asked for is to ask a
    /// serializer that refuses the other one. A length-prefixed binary sink
    /// would write the two differently.
    #[derive(Debug)]
    struct TupleOnly;

    impl fmt::Display for TupleOnly {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("this serializer accepts a tuple of numbers and nothing else")
        }
    }

    impl std::error::Error for TupleOnly {}

    impl ser::Error for TupleOnly {
        fn custom<T: fmt::Display>(_msg: T) -> Self {
            TupleOnly
        }
    }

    /// Counts the numbers of a tuple.
    struct TupleWriter {
        length: usize,
        written: usize,
    }

    impl SerializeTuple for TupleWriter {
        type Ok = (usize, usize);
        type Error = TupleOnly;

        fn serialize_element<T>(&mut self, _value: &T) -> Result<(), Self::Error>
        where
            T: ?Sized + serde::Serialize,
        {
            self.written += 1;
            Ok(())
        }

        fn end(self) -> Result<Self::Ok, Self::Error> {
            Ok((self.length, self.written))
        }
    }

    /// The methods this serializer refuses.
    macro_rules! refused {
        ($($name:ident($($arg:ident: $ty:ty),*);)*) => {
            $(
                fn $name(self $(, $arg: $ty)*) -> Result<Self::Ok, Self::Error> {
                    Err(TupleOnly)
                }
            )*
        };
    }

    /// The compound methods this serializer refuses.
    macro_rules! refused_compound {
        ($($name:ident -> $associated:ident($($arg:ident: $ty:ty),*);)*) => {
            $(
                fn $name(self $(, $arg: $ty)*) -> Result<Self::$associated, Self::Error> {
                    Err(TupleOnly)
                }
            )*
        };
    }

    impl serde::Serializer for TupleOnly {
        type Ok = (usize, usize);
        type Error = TupleOnly;
        type SerializeSeq = ser::Impossible<Self::Ok, Self::Error>;
        type SerializeTuple = TupleWriter;
        type SerializeTupleStruct = ser::Impossible<Self::Ok, Self::Error>;
        type SerializeTupleVariant = ser::Impossible<Self::Ok, Self::Error>;
        type SerializeMap = ser::Impossible<Self::Ok, Self::Error>;
        type SerializeStruct = ser::Impossible<Self::Ok, Self::Error>;
        type SerializeStructVariant = ser::Impossible<Self::Ok, Self::Error>;

        fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
            Ok(TupleWriter {
                length: len,
                written: 0,
            })
        }

        refused! {
            serialize_bool(_value: bool);
            serialize_i8(_value: i8);
            serialize_i16(_value: i16);
            serialize_i32(_value: i32);
            serialize_i64(_value: i64);
            serialize_u8(_value: u8);
            serialize_u16(_value: u16);
            serialize_u32(_value: u32);
            serialize_u64(_value: u64);
            serialize_f32(_value: f32);
            serialize_f64(_value: f64);
            serialize_char(_value: char);
            serialize_str(_value: &str);
            serialize_bytes(_value: &[u8]);
            serialize_none();
            serialize_unit();
            serialize_unit_struct(_name: &'static str);
            serialize_unit_variant(_name: &'static str, _variant_index: u32, _variant: &'static str);
        }

        refused_compound! {
            serialize_seq -> SerializeSeq(_len: Option<usize>);
            serialize_tuple_struct -> SerializeTupleStruct(_name: &'static str, _len: usize);
            serialize_tuple_variant -> SerializeTupleVariant(_name: &'static str, _variant_index: u32, _variant: &'static str, _len: usize);
            serialize_map -> SerializeMap(_len: Option<usize>);
            serialize_struct -> SerializeStruct(_name: &'static str, _len: usize);
            serialize_struct_variant -> SerializeStructVariant(_name: &'static str, _variant_index: u32, _variant: &'static str, _len: usize);
        }

        fn serialize_some<T>(self, _value: &T) -> Result<Self::Ok, Self::Error>
        where
            T: ?Sized + serde::Serialize,
        {
            Err(TupleOnly)
        }

        fn serialize_newtype_struct<T>(
            self,
            _name: &'static str,
            _value: &T,
        ) -> Result<Self::Ok, Self::Error>
        where
            T: ?Sized + serde::Serialize,
        {
            Err(TupleOnly)
        }

        fn serialize_newtype_variant<T>(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _value: &T,
        ) -> Result<Self::Ok, Self::Error>
        where
            T: ?Sized + serde::Serialize,
        {
            Err(TupleOnly)
        }
    }

    /// An affect vector is written as a tuple, not as a sequence.
    ///
    /// The two are the same under `serde_json`, and different under a sink
    /// that prefixes a length, so the shape the log holds must not depend on
    /// the format it is written in.
    #[test]
    fn an_affect_vector_is_written_as_a_tuple() {
        let (length, written) =
            serialize_finite_vector(&[0.1, 0.0, -0.5], TupleOnly).expect("a tuple must be written");
        assert_eq!(length, 3, "the vector is three numbers long");
        assert_eq!(written, 3, "every number of the vector must be written");
    }
}
