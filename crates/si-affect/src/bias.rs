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

/// One parameter the cognition loop reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitionParameter {
    Explore,
    RetrySame,
    Verify,
    Persist,
    Abandon,
    Risk,
    Ask,
    Connect,
    Create,
    Revise,
    Express,
    Refuse,
    Pace,
}

impl CognitionParameter {
    /// Every parameter, in registry order.
    pub const ALL: [CognitionParameter; 13] = [
        CognitionParameter::Explore,
        CognitionParameter::RetrySame,
        CognitionParameter::Verify,
        CognitionParameter::Persist,
        CognitionParameter::Abandon,
        CognitionParameter::Risk,
        CognitionParameter::Ask,
        CognitionParameter::Connect,
        CognitionParameter::Create,
        CognitionParameter::Revise,
        CognitionParameter::Express,
        CognitionParameter::Refuse,
        CognitionParameter::Pace,
    ];
}

impl fmt::Display for CognitionParameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            CognitionParameter::Explore => "explore",
            CognitionParameter::RetrySame => "retry_same",
            CognitionParameter::Verify => "verify",
            CognitionParameter::Persist => "persist",
            CognitionParameter::Abandon => "abandon",
            CognitionParameter::Risk => "risk",
            CognitionParameter::Ask => "ask",
            CognitionParameter::Connect => "connect",
            CognitionParameter::Create => "create",
            CognitionParameter::Revise => "revise",
            CognitionParameter::Express => "express",
            CognitionParameter::Refuse => "refuse",
            CognitionParameter::Pace => "pace",
        };
        f.write_str(name)
    }
}

/// The behaviour deltas currently in force, one per parameter.
///
/// All values are `0.0` by default: the affect layer has no opinion until a
/// feeling is actually felt.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CognitionParameters {
    // The length comes from the registry itself, so the two cannot drift
    // apart: a fourteenth parameter would not fit, and a removed one would
    // not leave a hole.
    values: [f32; CognitionParameter::ALL.len()],
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
