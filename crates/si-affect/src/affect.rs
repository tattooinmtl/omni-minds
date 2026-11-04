// Rust guideline compliant 2026-03-30

//! The continuous affect state.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The three-axis affect vector. Each axis is in `[-1, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affect {
    /// −1 displeasure, +1 pleasure.
    pub valence: f32,
    /// −1 calm, +1 activated.
    pub arousal: f32,
    /// −1 in control, +1 threatened or uncertain.
    pub tension: f32,
}

impl Default for Affect {
    fn default() -> Self {
        Affect {
            valence: 0.0,
            arousal: 0.0,
            tension: 0.0,
        }
    }
}

impl Affect {
    /// Adds a delta, clamping every axis to `[-1, 1]`.
    pub fn shift(&mut self, delta: [f32; 3]) {
        self.valence = (self.valence + delta[0]).clamp(-1.0, 1.0);
        self.arousal = (self.arousal + delta[1]).clamp(-1.0, 1.0);
        self.tension = (self.tension + delta[2]).clamp(-1.0, 1.0);
    }

    /// Decays toward `baseline` exponentially over `elapsed`.
    ///
    /// `tau` is the decay time constant: after one `tau` the distance to the
    /// baseline has shrunk to 1/e of what it was.
    pub fn decay_toward(&mut self, baseline: Affect, elapsed: Duration, tau: Duration) {
        let tau_secs = tau.as_secs_f32();
        if tau_secs <= 0.0 || elapsed.as_secs_f32() <= 0.0 {
            return;
        }
        let factor = (-elapsed.as_secs_f32() / tau_secs).exp();
        self.valence = baseline.valence + (self.valence - baseline.valence) * factor;
        self.arousal = baseline.arousal + (self.arousal - baseline.arousal) * factor;
        self.tension = baseline.tension + (self.tension - baseline.tension) * factor;
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
    pub fn from_array(values: [f32; 3]) -> Self {
        Affect {
            valence: values[0],
            arousal: values[1],
            tension: values[2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
        let tau = Duration::from_secs(60);
        affect.decay_toward(Affect::default(), Duration::from_secs(60), tau);
        assert!(
            (affect.valence - 1.0 / std::f32::consts::E).abs() < 0.001,
            "one time constant leaves 1/e of the value"
        );

        let mut affect = Affect {
            valence: 1.0,
            arousal: 0.0,
            tension: 0.0,
        };
        affect.decay_toward(Affect::default(), Duration::from_secs(0), tau);
        assert_eq!(affect.valence, 1.0, "no elapsed time means no decay");
    }
}
