// Rust guideline compliant 2026-03-30

//! The continuous affect state.
//!
//! Three axes, each in `[-1, 1]`, that move when a feeling is felt and decay
//! back toward the baseline when nothing is happening. The state is derived
//! from the append-only event log, so every value that enters it is brought
//! into range: one poisoned axis would be permanent, because the log is
//! never rewritten.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Duration;

/// The lowest an axis may hold.
///
/// −1 is the end of the scale the 250 feelings are written against
/// (`data/feelings/*.yaml`), and the live state has to stay inside the same
/// space for a nearest-neighbour label to mean anything. Widening this bound
/// would let the live state drift outside the space the labels were defined
/// in, and the labelling would silently stop describing the state.
///
/// The lexicon loader checks its canonical points against the same bound, so
/// the labels and the state they are matched against share one range.
pub(crate) const AXIS_MIN: f32 = -1.0;

/// The highest an axis may hold.
///
/// The other end of the same scale as [`AXIS_MIN`], and closed for the same
/// reason: the state and the labels share one space.
pub(crate) const AXIS_MAX: f32 = 1.0;

/// The value an axis holds when nothing is happening.
///
/// This is the neutral state the agent returns to when no feeling is felt, and
/// the value a number that carries no information falls back to.
const AXIS_NEUTRAL: f32 = 0.0;

/// Brings one axis into range.
///
/// A value that is not a number carries no information, so it becomes the
/// neutral zero rather than poisoning every later calculation. The affect
/// state is derived from an append-only log, so one NaN would be permanent:
/// it would survive every decay, and it would make its own event unreadable.
fn clamp_axis(value: f32) -> f32 {
    if value.is_nan() {
        AXIS_NEUTRAL
    } else {
        value.clamp(AXIS_MIN, AXIS_MAX)
    }
}

/// Adds one delta to an axis, keeping the axis in range.
///
/// A delta that is not a number carries no information, so the axis does not
/// move: a mood does not reset because a bad number arrived. The axis is
/// still brought into range, so a state that is already a number stays one.
fn shifted(current: f32, delta: f32) -> f32 {
    if delta.is_nan() {
        clamp_axis(current)
    } else {
        clamp_axis(current + delta)
    }
}

/// Writes one axis, brought into range.
///
/// The log is append-only, so what is written once is permanent. An axis that
/// is out of range, or is not a number at all, is therefore written as the
/// in-range value it stands for, and the state that comes back out of the
/// log is the state that was written.
fn serialize_axis<S>(value: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_f32(clamp_axis(*value))
}

/// Reads one axis, brought into range.
///
/// A number outside the range is clamped. A value that is not a number at all
/// — which is how `serde_json` writes a NaN — becomes the neutral zero.
/// Either way a malformed axis is repaired rather than poisoning the state
/// or failing the read.
fn deserialize_axis<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = match Option::<f32>::deserialize(deserializer)? {
        Some(value) => clamp_axis(value),
        None => AXIS_NEUTRAL,
    };
    Ok(value)
}

/// The three-axis affect vector. Each axis is in `[-1, 1]`.
///
/// The range is enforced wherever a state is built, changed, written or read:
/// a value that is out of range, infinite, or not a number is brought into
/// range rather than kept. The affect state is derived from an append-only
/// log, so a value written once is permanent — one poisoned axis would
/// corrupt every later derivation of the state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affect {
    /// −1 displeasure, +1 pleasure.
    #[serde(
        serialize_with = "serialize_axis",
        deserialize_with = "deserialize_axis"
    )]
    pub valence: f32,
    /// −1 calm, +1 activated.
    #[serde(
        serialize_with = "serialize_axis",
        deserialize_with = "deserialize_axis"
    )]
    pub arousal: f32,
    /// −1 in control, +1 threatened or uncertain.
    #[serde(
        serialize_with = "serialize_axis",
        deserialize_with = "deserialize_axis"
    )]
    pub tension: f32,
}

impl Default for Affect {
    fn default() -> Self {
        Affect {
            valence: AXIS_NEUTRAL,
            arousal: AXIS_NEUTRAL,
            tension: AXIS_NEUTRAL,
        }
    }
}

impl Affect {
    /// Adds a delta, clamping every axis to `[-1, 1]`.
    ///
    /// A delta that is not a number carries no information, so that axis does
    /// not move at all.
    pub fn shift(&mut self, delta: [f32; 3]) {
        self.valence = shifted(self.valence, delta[0]);
        self.arousal = shifted(self.arousal, delta[1]);
        self.tension = shifted(self.tension, delta[2]);
    }

    /// Decays toward `baseline` exponentially over `elapsed`.
    ///
    /// `tau` is the decay time constant: after one `tau` the distance to the
    /// baseline has shrunk to 1/e of what it was.
    ///
    /// The result is brought into range. A baseline outside the range is a
    /// caller error, but the state it produces still has to be a state the
    /// loop can reason about.
    pub fn decay_toward(&mut self, baseline: Affect, elapsed: Duration, tau: Duration) {
        let tau_secs = tau.as_secs_f32();
        if tau_secs <= 0.0 || elapsed.as_secs_f32() <= 0.0 {
            return;
        }
        let factor = (-elapsed.as_secs_f32() / tau_secs).exp();
        self.valence = clamp_axis(baseline.valence + (self.valence - baseline.valence) * factor);
        self.arousal = clamp_axis(baseline.arousal + (self.arousal - baseline.arousal) * factor);
        self.tension = clamp_axis(baseline.tension + (self.tension - baseline.tension) * factor);
    }

    /// Euclidean distance to another affect state.
    pub fn distance(&self, other: Affect) -> f32 {
        let dv = self.valence - other.valence;
        let da = self.arousal - other.arousal;
        let dt = self.tension - other.tension;
        (dv * dv + da * da + dt * dt).sqrt()
    }

    /// The axes as an array, for storage in an event.
    pub fn to_array(&self) -> [f32; 3] {
        [self.valence, self.arousal, self.tension]
    }

    /// The axes from an array.
    ///
    /// The array is brought into range: an affect state that is out of range
    /// is not a state the loop can reason about.
    pub fn from_array(values: [f32; 3]) -> Self {
        Affect {
            valence: clamp_axis(values[0]),
            arousal: clamp_axis(values[1]),
            tension: clamp_axis(values[2]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// One time constant, the unit every decay in this module is measured in.
    const TAU: Duration = Duration::from_secs(60);

    #[test]
    fn shifting_clamps_to_the_unit_range() {
        let mut affect = Affect::default();
        affect.shift([2.0, -2.0, 0.5]);
        assert_eq!(affect.valence, 1.0);
        assert_eq!(affect.arousal, -1.0);
        assert_eq!(affect.tension, 0.5);
    }

    #[test]
    fn decay_moves_toward_baseline_exponentially() {
        let mut affect = Affect {
            valence: 1.0,
            arousal: 0.0,
            tension: 0.0,
        };
        affect.decay_toward(Affect::default(), Duration::from_secs(60), TAU);
        assert!(
            (affect.valence - 1.0 / std::f32::consts::E).abs() < 0.001,
            "one time constant leaves 1/e of the value"
        );

        let mut affect = Affect {
            valence: 1.0,
            arousal: 0.0,
            tension: 0.0,
        };
        affect.decay_toward(Affect::default(), Duration::from_secs(0), TAU);
        assert_eq!(affect.valence, 1.0, "no elapsed time means no decay");
    }

    #[test]
    fn decay_moves_toward_the_baseline_and_not_toward_zero() {
        let baseline = Affect {
            valence: 0.5,
            arousal: 0.0,
            tension: 0.0,
        };
        let mut affect = Affect {
            valence: 1.0,
            arousal: 0.0,
            tension: 0.0,
        };
        affect.decay_toward(baseline, TAU, TAU);
        let expected = 0.5 + 0.5 / std::f32::consts::E;
        assert!(
            (affect.valence - expected).abs() < 0.001,
            "one time constant leaves 1/e of the distance to the baseline, got {}",
            affect.valence
        );
        assert!(
            affect.valence > baseline.valence,
            "the decay must approach the baseline, not fall through it toward zero"
        );
    }

    #[test]
    fn decay_moves_every_axis() {
        let mut affect = Affect {
            valence: 1.0,
            arousal: 0.6,
            tension: -0.8,
        };
        affect.decay_toward(Affect::default(), TAU, TAU);
        assert!(
            (affect.valence - 1.0 / std::f32::consts::E).abs() < 0.001,
            "valence must decay"
        );
        assert!(
            (affect.arousal - 0.6 / std::f32::consts::E).abs() < 0.001,
            "arousal must decay too"
        );
        assert!(
            (affect.tension + 0.8 / std::f32::consts::E).abs() < 0.001,
            "tension must decay too"
        );
    }

    #[test]
    fn decay_toward_a_non_zero_baseline_moves_every_axis() {
        let baseline = Affect {
            valence: 0.5,
            arousal: -0.2,
            tension: 0.3,
        };
        let current = Affect {
            valence: 1.0,
            arousal: 0.6,
            tension: -0.8,
        };
        let mut affect = current;
        affect.decay_toward(baseline, TAU, TAU);
        for (axis, expected) in [
            (
                affect.valence,
                baseline.valence + (current.valence - baseline.valence) / std::f32::consts::E,
            ),
            (
                affect.arousal,
                baseline.arousal + (current.arousal - baseline.arousal) / std::f32::consts::E,
            ),
            (
                affect.tension,
                baseline.tension + (current.tension - baseline.tension) / std::f32::consts::E,
            ),
        ] {
            assert!(
                (axis - expected).abs() < 0.001,
                "every axis must decay toward the baseline, got {axis} expected {expected}"
            );
        }
    }

    #[test]
    fn a_decay_with_no_time_constant_changes_nothing() {
        let mut affect = Affect {
            valence: 0.5,
            arousal: -0.5,
            tension: 0.25,
        };
        affect.decay_toward(Affect::default(), TAU, Duration::from_secs(0));
        assert_eq!(
            affect,
            Affect {
                valence: 0.5,
                arousal: -0.5,
                tension: 0.25
            },
            "a zero time constant means no decay"
        );
    }

    #[test]
    fn a_very_long_decay_lands_exactly_on_the_baseline() {
        let baseline = Affect {
            valence: 0.5,
            arousal: -0.5,
            tension: 0.25,
        };
        let mut affect = Affect {
            valence: 1.0,
            arousal: 0.0,
            tension: -1.0,
        };
        affect.decay_toward(
            baseline,
            Duration::from_secs(60 * 60 * 24),
            Duration::from_secs(1),
        );
        assert_eq!(affect, baseline, "the decay lands exactly on the baseline");
    }

    #[test]
    fn a_delta_that_is_not_a_number_leaves_the_axis_alone() {
        let mut affect = Affect {
            valence: 0.4,
            arousal: 0.2,
            tension: -0.3,
        };
        affect.shift([f32::NAN, f32::NAN, 0.1]);
        assert_eq!(affect.valence, 0.4, "a NaN delta carries no information");
        assert_eq!(affect.arousal, 0.2, "a NaN delta carries no information");
        assert!(
            (affect.tension + 0.2).abs() < 0.000_001,
            "the other axes still move, got {}",
            affect.tension
        );
        assert!(
            affect.distance(Affect::default()).is_finite(),
            "the state must stay a number"
        );
    }

    #[test]
    fn an_infinite_delta_saturates_the_axis() {
        let mut affect = Affect::default();
        affect.shift([f32::INFINITY, f32::NEG_INFINITY, 0.0]);
        assert_eq!(affect.valence, 1.0);
        assert_eq!(affect.arousal, -1.0);
        assert_eq!(affect.tension, 0.0);
    }

    #[test]
    fn the_state_cannot_be_pushed_out_of_range() {
        let affect = Affect::from_array([2.0, -2.0, 0.5]);
        assert_eq!(affect.valence, 1.0);
        assert_eq!(affect.arousal, -1.0);
        assert_eq!(affect.tension, 0.5);

        let mut affect = Affect {
            valence: 0.0,
            arousal: 0.0,
            tension: 0.0,
        };
        affect.decay_toward(
            Affect {
                valence: 5.0,
                arousal: -5.0,
                tension: 5.0,
            },
            TAU,
            TAU,
        );
        assert_eq!(affect.valence, 1.0, "the decay must not leave the range");
        assert_eq!(affect.arousal, -1.0, "the decay must not leave the range");
        assert_eq!(affect.tension, 1.0, "the decay must not leave the range");
    }

    #[test]
    fn the_axes_survive_a_round_trip_through_an_array() {
        let affect = Affect {
            valence: 0.5,
            arousal: -0.25,
            tension: 1.0,
        };
        assert_eq!(Affect::from_array(affect.to_array()), affect);
    }

    #[test]
    fn the_distance_between_two_states_is_euclidean() {
        let a = Affect {
            valence: 0.0,
            arousal: 0.0,
            tension: 0.0,
        };
        let b = Affect {
            valence: 0.3,
            arousal: 0.4,
            tension: 0.0,
        };
        assert!(
            (a.distance(b) - 0.5).abs() < 0.000_001,
            "the distance is the euclidean distance between the axes, got {}",
            a.distance(b)
        );
        assert_eq!(a.distance(b), b.distance(a), "the distance is symmetric");
    }

    #[test]
    fn a_state_survives_a_serde_round_trip() {
        let affect = Affect {
            valence: 0.5,
            arousal: -0.25,
            tension: 1.0,
        };
        let json = serde_json::to_string(&affect).expect("serialise");
        assert_eq!(
            json, r#"{"valence":0.5,"arousal":-0.25,"tension":1.0}"#,
            "the wire format is plain numbers"
        );
        let read: Affect = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(read, affect);
    }

    #[test]
    fn an_out_of_range_axis_is_clamped_on_the_way_in() {
        let read: Affect = serde_json::from_str(r#"{"valence":5.0,"arousal":-5.0,"tension":0.25}"#)
            .expect("deserialise");
        assert_eq!(read.valence, 1.0);
        assert_eq!(read.arousal, -1.0);
        assert_eq!(read.tension, 0.25);
    }

    #[test]
    fn a_non_finite_axis_is_brought_into_range_on_the_way_in() {
        // `serde_json` writes a NaN as `null`, and `null` is not a number.
        let read: Affect =
            serde_json::from_str(r#"{"valence":null,"arousal":null,"tension":null}"#)
                .expect("deserialise");
        assert_eq!(read.valence, 0.0, "a value that is not a number is neutral");
        assert_eq!(read.arousal, 0.0);
        assert_eq!(read.tension, 0.0);
    }

    #[test]
    fn a_state_with_a_bad_number_is_written_in_range() {
        // The one way a bad number can still enter: a caller writing the
        // field directly. The log must still hold a state that reads back.
        let affect = Affect {
            valence: f32::NAN,
            arousal: 5.0,
            tension: -5.0,
        };
        let json = serde_json::to_string(&affect).expect("serialise");
        assert!(
            !json.contains("null"),
            "a non-finite axis must never reach the log as a null, got {json}"
        );
        let read: Affect = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(
            read.valence, 0.0,
            "a NaN axis is written as the neutral zero"
        );
        assert_eq!(read.arousal, 1.0);
        assert_eq!(read.tension, -1.0);
    }
}
