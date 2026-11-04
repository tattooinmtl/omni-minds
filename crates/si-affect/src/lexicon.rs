// Rust guideline compliant 2026-03-30

//! The 250-feeling lexicon.

use std::collections::BTreeMap;

use crate::affect::Affect;
use crate::bias::CognitionParameter;
use crate::feeling::Feeling;
use thiserror::Error;

/// The raw YAML shape of one feeling, before bias keys are resolved.
#[derive(Debug, serde::Deserialize)]
struct RawFeeling {
    id: String,
    name: String,
    family: String,
    #[serde(default)]
    definition: String,
    valence: f32,
    arousal: f32,
    tension: f32,
    #[serde(default)]
    bias: BTreeMap<String, f32>,
    #[serde(default)]
    nearest: Vec<String>,
}

/// Failures loading the lexicon.
#[derive(Debug, Error)]
pub enum AffectError {
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("feeling {0} names a bias parameter outside the registry: {1}")]
    UnknownBiasParameter(String, String),
    #[error("the lexicon is empty")]
    Empty,
}

/// A labelled reading of an affect state.
#[derive(Debug, Clone)]
pub struct FeelingMatch {
    pub primary: Feeling,
    pub runner_up: Option<Feeling>,
    /// Distance from the affect state to the primary label.
    pub distance: f32,
}

/// Every feeling, loaded from the embedded YAML.
#[derive(Debug, Clone)]
pub struct Lexicon {
    pub feelings: Vec<Feeling>,
}

impl Lexicon {
    /// Loads the lexicon compiled into the binary, so the mind never depends on
    /// a file being present at runtime.
    pub fn load_embedded() -> Result<Self, AffectError> {
        let mut feelings = Vec::new();
        for source in EMBEDDED {
            let parsed: Vec<RawFeeling> = serde_yaml::from_str(source)?;
            for raw in parsed {
                let mut bias = BTreeMap::new();
                for (key, value) in raw.bias {
                    let parameter = parse_parameter(&key).ok_or_else(|| {
                        AffectError::UnknownBiasParameter(raw.id.clone(), key.clone())
                    })?;
                    bias.insert(parameter, value);
                }
                feelings.push(Feeling {
                    id: raw.id,
                    name: raw.name,
                    family: raw.family,
                    definition: raw.definition,
                    valence: raw.valence,
                    arousal: raw.arousal,
                    tension: raw.tension,
                    bias,
                    nearest: raw.nearest,
                });
            }
        }
        if feelings.is_empty() {
            return Err(AffectError::Empty);
        }
        Ok(Lexicon { feelings })
    }

    /// Number of feelings.
    pub fn len(&self) -> usize {
        self.feelings.len()
    }

    /// Whether the lexicon is empty.
    pub fn is_empty(&self) -> bool {
        self.feelings.is_empty()
    }

    /// Finds a feeling by id.
    pub fn get(&self, id: &str) -> Option<&Feeling> {
        self.feelings.iter().find(|f| f.id == id)
    }

    /// Labels an affect state.
    ///
    /// The runner-up is always reported, because a feeling is only meaningful
    /// relative to the one it nearly was.
    pub fn label(&self, affect: Affect) -> FeelingMatch {
        let mut ranked: Vec<(&Feeling, f32)> = self
            .feelings
            .iter()
            .map(|f| (f, affect.distance(f.point())))
            .collect();
        ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (primary, distance) = ranked[0];
        let runner_up = ranked.get(1).map(|(f, _)| (*f).clone());
        FeelingMatch {
            primary: primary.clone(),
            runner_up,
            distance,
        }
    }
}

/// Resolves a YAML bias key to its registry parameter.
fn parse_parameter(key: &str) -> Option<CognitionParameter> {
    CognitionParameter::ALL
        .iter()
        .copied()
        .find(|p| p.to_string() == key)
}

/// The lexicon YAML, embedded at build time.
const EMBEDDED: &[&str] = &[
    include_str!("../data/feelings/01-positive-activation.yaml"),
    include_str!("../data/feelings/02-positive-calm.yaml"),
    include_str!("../data/feelings/03-negative-activation.yaml"),
    include_str!("../data/feelings/04-negative-low.yaml"),
    include_str!("../data/feelings/05-epistemic.yaml"),
    include_str!("../data/feelings/06-effort.yaml"),
    include_str!("../data/feelings/07-relational.yaml"),
    include_str!("../data/feelings/08-moral-self.yaml"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affect::Affect;

    #[test]
    fn the_embedded_lexicon_holds_exactly_250_feelings() {
        let lexicon = Lexicon::load_embedded().expect("load");
        assert_eq!(lexicon.len(), 250);
    }

    #[test]
    fn every_feeling_has_a_non_empty_bias_within_the_registry() {
        let lexicon = Lexicon::load_embedded().expect("load");
        for feeling in &lexicon.feelings {
            assert!(!feeling.bias.is_empty(), "{} has an empty bias", feeling.id);
            for (parameter, value) in &feeling.bias {
                assert!(
                    (-1.0..=1.0).contains(value),
                    "{} bias on {} is out of range",
                    feeling.id,
                    parameter
                );
            }
        }
    }

    #[test]
    fn labelling_reports_the_runner_up_and_a_distance() {
        let lexicon = Lexicon::load_embedded().expect("load");
        let affect = Affect {
            valence: -0.45,
            arousal: 0.60,
            tension: 0.70,
        };
        let matched = lexicon.label(affect);
        assert_eq!(matched.primary.id, "frustration");
        assert!(
            matched.runner_up.is_some(),
            "a reading must always carry a runner-up"
        );
        assert!(
            matched.distance < 0.01,
            "the exact canonical point is at distance zero"
        );
    }

    #[test]
    fn a_blend_lands_between_its_two_closest_labels() {
        let lexicon = Lexicon::load_embedded().expect("load");
        // The midpoint of joy (0.80, 0.65, -0.35) and mirth (0.75, 0.65, -0.25).
        // These two name each other as nearest and no other label sits between
        // them, so the midpoint must resolve to exactly this pair.
        let blend = Affect {
            valence: 0.775,
            arousal: 0.65,
            tension: -0.30,
        };
        let matched = lexicon.label(blend);
        let ids = [
            matched.primary.id.clone(),
            matched.runner_up.map(|r| r.id).unwrap_or_default(),
        ];
        assert!(
            ids.contains(&"joy".to_string()) && ids.contains(&"mirth".to_string()),
            "a midpoint must resolve to the two labels it sits between, got {ids:?}"
        );
    }
}
