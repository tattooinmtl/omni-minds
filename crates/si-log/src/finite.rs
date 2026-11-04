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

use serde::de;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serializer};

/// The value a number that is not finite is repaired to.
///
/// A number that is not finite carries no information — no magnitude, no
/// direction — so the log keeps the neutral zero. Zero is what `si-affect`
/// gives an axis that is not a number, and what the read path gives a
/// `null`, so a repaired number and a number that was never there look the
/// same in the log.
const NON_FINITE_DEFAULT: f32 = 0.0;

/// The number of axes in an affect vector.
///
/// The affect state is three axes wide (`valence`, `arousal`, `tension`), so
/// a vector written into an event is three numbers long. A vector of another
/// length is a malformed event, not a repaired one.
const VECTOR_LEN: usize = 3;

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

/// Repairs an affect vector so it can be written to the log.
///
/// Every axis is repaired on its own, so a vector that is already finite
/// comes back unchanged.
pub(crate) fn finite_vector(values: [f32; 3]) -> [f32; 3] {
    [finite(values[0]), finite(values[1]), finite(values[2])]
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

/// Reads one number, repaired.
///
/// `serde_json` writes a number that is not finite as `null`, and `null` is
/// not a valid `f32`, so the value is read as an optional number: a `null`
/// becomes the neutral default instead of failing the whole row. A number
/// that overflows `f32` is read as an infinity and repaired the same way.
///
/// A missing field is still an error. Only a malformed number is repaired
/// here; an absent one is absent data, and inventing it would put a number
/// into the log that nobody wrote.
pub(crate) fn deserialize_finite<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<f32>::deserialize(deserializer)? {
        Some(value) => Ok(finite(value)),
        None => Ok(NON_FINITE_DEFAULT),
    }
}

/// Writes an affect vector, repaired.
///
/// The vector is written as a plain array of three numbers, which is the
/// shape the log has always held.
pub(crate) fn serialize_finite_vector<S>(
    values: &[f32; VECTOR_LEN],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut seq = serializer.serialize_seq(Some(VECTOR_LEN))?;
    for axis in values {
        seq.serialize_element(&finite(*axis))?;
    }
    seq.end()
}

/// Reads an affect vector, repaired.
///
/// The vector is read as a list of optional numbers, so an axis that was
/// written as `null` becomes the neutral default instead of failing the
/// whole row.
///
/// A vector of the wrong length is still an error: a missing axis is missing
/// data, not a malformed number.
pub(crate) fn deserialize_finite_vector<'de, D>(deserializer: D) -> Result<[f32; 3], D::Error>
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
    Ok([
        finite(axes[0].unwrap_or(NON_FINITE_DEFAULT)),
        finite(axes[1].unwrap_or(NON_FINITE_DEFAULT)),
        finite(axes[2].unwrap_or(NON_FINITE_DEFAULT)),
    ])
}
