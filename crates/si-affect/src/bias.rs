// Rust guideline compliant 2026-03-30

//! The closed registry of cognition parameters.
//!
//! A feeling may only bias parameters listed here. Keeping the set closed is
//! what stops the lexicon and the loop from drifting apart: a bias naming a
//! parameter the loop does not read would be a no-op, and the anti-wallpaper
//! rule would be unenforceable.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Declares the registry once and expands it into everything that has to agree
/// with it: the enum, the list of every parameter, the count, and the name
/// each parameter prints and serialises as.
///
/// One list is the point. The values array is indexed by discriminant, so a
/// variant that was not in the list would index past the end of that array —
/// a runtime panic in the middle of the cognition loop, with nothing to
/// explain it. Declaring the variants once makes that impossible rather than
/// merely discouraged.
macro_rules! declare_parameters {
    ($($variant:ident => $spelling:literal),+ $(,)?) => {
        /// One parameter the cognition loop reads.
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub enum CognitionParameter {
            $(
                #[serde(rename = $spelling)]
                $variant,
            )+
        }

        impl CognitionParameter {
            /// Every parameter, in registry order.
            pub const ALL: [CognitionParameter; Self::COUNT] = [$(Self::$variant),+];

            /// How many parameters the registry holds.
            pub const COUNT: usize = {
                let mut count = 0usize;
                $( {
                    let _ = Self::$variant;
                    count += 1;
                } )+
                count
            };
        }

        impl fmt::Display for CognitionParameter {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let name = match self {
                    $( Self::$variant => $spelling, )+
                };
                f.write_str(name)
            }
        }
    };
}

declare_parameters!(
    Explore => "explore",
    RetrySame => "retry_same",
    Verify => "verify",
    Persist => "persist",
    Abandon => "abandon",
    Risk => "risk",
    Ask => "ask",
    Connect => "connect",
    Create => "create",
    Revise => "revise",
    Express => "express",
    Refuse => "refuse",
    Pace => "pace",
);

/// The behaviour deltas currently in force, one per parameter.
///
/// All values are `0.0` by default: the affect layer has no opinion until a
/// feeling is actually felt.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CognitionParameters {
    // The length comes from the registry itself, so the two cannot drift
    // apart: a fourteenth parameter would not fit, and a removed one would
    // not leave a hole.
    values: [f32; CognitionParameter::COUNT],
}

impl CognitionParameters {
    /// The value for one parameter.
    pub fn get(&self, parameter: CognitionParameter) -> f32 {
        self.values[parameter as usize]
    }

    /// Overwrites one parameter.
    pub fn set(&mut self, parameter: CognitionParameter, value: f32) {
        self.values[parameter as usize] = value;
    }

    /// Applies a bias map, scaled by `scale`, on top of the current values.
    pub fn apply(&mut self, bias: &BTreeMap<CognitionParameter, f32>, scale: f32) {
        for (parameter, delta) in bias {
            let index = *parameter as usize;
            self.values[index] += delta * scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn the_registry_has_exactly_thirteen_parameters() {
        assert_eq!(CognitionParameter::ALL.len(), 13);
        assert_eq!(CognitionParameter::COUNT, 13);
    }

    #[test]
    fn the_list_is_in_discriminant_order() {
        // The values array is indexed by discriminant, so the registry's order
        // is the array's order. If they drift, a parameter reads another's
        // value.
        for (index, parameter) in CognitionParameter::ALL.iter().enumerate() {
            assert_eq!(
                *parameter as usize, index,
                "the registry must be in discriminant order"
            );
        }
    }

    #[test]
    fn applying_a_bias_moves_only_the_named_parameters() {
        let mut params = CognitionParameters::default();
        let mut bias = BTreeMap::new();
        bias.insert(CognitionParameter::Explore, -0.3);
        bias.insert(CognitionParameter::RetrySame, 0.4);
        params.apply(&bias, 1.0);
        assert_eq!(params.get(CognitionParameter::Explore), -0.3);
        assert_eq!(params.get(CognitionParameter::RetrySame), 0.4);
        assert_eq!(params.get(CognitionParameter::Verify), 0.0);
    }

    #[test]
    fn scale_dampens_the_bias() {
        let mut params = CognitionParameters::default();
        let mut bias = BTreeMap::new();
        bias.insert(CognitionParameter::Verify, 0.5);
        params.apply(&bias, 0.5);
        assert_eq!(params.get(CognitionParameter::Verify), 0.25);
    }
}
