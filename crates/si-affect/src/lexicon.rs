// Rust guideline compliant 2026-03-30

//! The 250-feeling lexicon.

use std::collections::BTreeMap;

use crate::affect::{Affect, AXIS_MAX, AXIS_MIN};
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
    /// The YAML could not be read at all.
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    /// A bias named a parameter the cognition loop does not read.
    #[error("feeling {0} names a bias parameter outside the registry: {1}")]
    UnknownBiasParameter(String, String),
    /// A feeling with no bias cannot move behaviour, so it is a label and not
    /// a feeling.
    #[error("feeling {0} has an empty bias - a feeling that cannot move behaviour is a label, not a feeling")]
    EmptyBias(String),
    /// A canonical point outside the range the affect space is written in.
    #[error("feeling {0} has {1}={2}, outside the axis range {3:?}")]
    AxisOutOfRange(String, String, f32, (f32, f32)),
    /// The lexicon holds no feelings at all.
    #[error("the lexicon is empty")]
    Empty,
}

/// A labelled reading of an affect state.
#[derive(Debug, Clone)]
pub struct FeelingMatch {
    /// The feeling the state sits closest to.
    pub primary: Feeling,
    /// The feeling it nearly was, when the lexicon has more than one.
    pub runner_up: Option<Feeling>,
    /// Distance from the affect state to the primary label.
    pub distance: f32,
    /// Distance from the affect state to the runner-up label, when there is
    /// one. The spec's example prints both distances, because a reading that
    /// names only one label cannot be judged against the other.
    pub runner_up_distance: Option<f32>,
}

/// Every feeling, loaded from the embedded YAML.
#[derive(Debug, Clone)]
pub struct Lexicon {
    /// The feelings, in embedded order.
    ///
    /// Private on purpose: a lexicon built outside the crate could be empty,
    /// and an empty lexicon has no label to report. Nothing outside this crate
    /// constructs a `Lexicon`.
    feelings: Vec<Feeling>,
}

impl Lexicon {
    /// Loads the lexicon compiled into the binary, so the mind never depends on
    /// a file being present at runtime.
    pub fn load_embedded() -> Result<Self, AffectError> {
        Self::load_sources(EMBEDDED)
    }

    /// Loads a lexicon from YAML sources, in embedded order.
    ///
    /// The loader refuses a lexicon it cannot trust. An empty lexicon has no
    /// label to report; a feeling with no bias cannot move behaviour, so it is
    /// a label and not a feeling; a bias naming a parameter the loop does not
    /// read is a no-op; and a canonical point outside the range the affect
    /// space is written in would put a feeling somewhere no state can reach.
    fn load_sources(sources: &[&str]) -> Result<Self, AffectError> {
        let mut feelings = Vec::new();
        for source in sources {
            let parsed: Vec<RawFeeling> = serde_yaml::from_str(source)?;
            for raw in parsed {
                if raw.bias.is_empty() {
                    return Err(AffectError::EmptyBias(raw.id.clone()));
                }
                check_axis(&raw.id, "valence", raw.valence)?;
                check_axis(&raw.id, "arousal", raw.arousal)?;
                check_axis(&raw.id, "tension", raw.tension)?;

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

    /// The feelings, in embedded order.
    pub fn feelings(&self) -> &[Feeling] {
        &self.feelings
    }

    /// Finds a feeling by id.
    pub fn get(&self, id: &str) -> Option<&Feeling> {
        self.feelings.iter().find(|f| f.id == id)
    }

    /// Labels an affect state.
    ///
    /// The runner-up is always reported, because a feeling is only meaningful
    /// relative to the one it nearly was.
    ///
    /// Ties are broken by the position of the feeling in the embedded lexicon:
    /// the sort is stable, so two feelings at the same distance come back in
    /// the order they were loaded — file order, then line order. The lexicon
    /// this crate ships has no two feelings at the same canonical point, and
    /// `tools/validate_lexicon.py` refuses a lexicon in which one does, so a
    /// tie between two points cannot arise in the data. A caller that hands
    /// `label` a lexicon of its own still gets this documented order rather
    /// than an arbitrary one.
    pub fn label(&self, affect: Affect) -> FeelingMatch {
        let mut ranked: Vec<(&Feeling, f32)> = self
            .feelings
            .iter()
            .map(|f| (f, affect.distance(f.point())))
            .collect();
        ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
        // The lexicon is never empty: the loader refuses one, and the field is
        // private, so no caller can hand `label` an empty space to label. An
        // empty one would be a programming error, not a state to report.
        debug_assert!(
            !ranked.is_empty(),
            "a lexicon with no feelings has no label to report"
        );
        let (primary, distance) = ranked[0];
        let runner_up = ranked.get(1).map(|(f, _)| (*f).clone());
        let runner_up_distance = ranked.get(1).map(|(_, d)| *d);
        FeelingMatch {
            primary: primary.clone(),
            runner_up,
            distance,
            runner_up_distance,
        }
    }
}

/// Checks one canonical axis against the range the affect space is written in.
fn check_axis(id: &str, axis: &str, value: f32) -> Result<(), AffectError> {
    if !(AXIS_MIN..=AXIS_MAX).contains(&value) {
        return Err(AffectError::AxisOutOfRange(
            id.to_string(),
            axis.to_string(),
            value,
            (AXIS_MIN, AXIS_MAX),
        ));
    }
    Ok(())
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
    use std::collections::BTreeMap;

    /// How many feelings the lexicon holds.
    ///
    /// 250 is the number the design fixes, across the eight families the YAML
    /// ships. It is written down in one place so a family that gains or loses
    /// a feeling has to change this constant too.
    const EXPECTED_FEELING_COUNT: usize = 250;

    /// How close a label has to be to the state it labels.
    ///
    /// A state that sits exactly on a canonical point is at distance zero. The
    /// tolerance only absorbs the rounding of the two-decimal coordinates the
    /// YAML holds, so a label that is merely nearby cannot pass for an exact
    /// one.
    const EXACT_POINT_TOLERANCE: f32 = 0.01;

    /// One feeling whose bias key is not in the registry.
    const LEXICON_WITH_UNKNOWN_BIAS_KEY: &str = "- id: rogue\n  name: Rogue\n  family: test\n  definition: A feeling that names a parameter the loop does not read.\n  valence: 0.0\n  arousal: 0.0\n  tension: 0.0\n  bias: {explore: 0.1, not_a_parameter: 0.2}\n  nearest: [rogue]\n";

    /// One feeling with no bias at all.
    const LEXICON_WITH_EMPTY_BIAS: &str = "- id: hollow\n  name: Hollow\n  family: test\n  definition: A feeling that cannot move behaviour.\n  valence: 0.0\n  arousal: 0.0\n  tension: 0.0\n  bias: {}\n  nearest: [hollow]\n";

    /// One feeling whose canonical point sits outside the affect space.
    const LEXICON_WITH_OUT_OF_RANGE_AXIS: &str = "- id: wild\n  name: Wild\n  family: test\n  definition: A feeling placed outside the range the space is written in.\n  valence: 1.5\n  arousal: 0.0\n  tension: 0.0\n  bias: {explore: 0.1}\n  nearest: [wild]\n";

    #[test]
    fn the_embedded_lexicon_holds_exactly_250_feelings() {
        let lexicon = Lexicon::load_embedded().expect("load");
        assert_eq!(lexicon.len(), EXPECTED_FEELING_COUNT);
        assert_eq!(lexicon.feelings().len(), EXPECTED_FEELING_COUNT);
    }

    #[test]
    fn every_feeling_carries_a_non_empty_bias() {
        let lexicon = Lexicon::load_embedded().expect("load");
        for feeling in lexicon.feelings() {
            assert!(!feeling.bias.is_empty(), "{} has an empty bias", feeling.id);
        }
    }

    #[test]
    fn every_bias_value_is_within_the_unit_range() {
        let lexicon = Lexicon::load_embedded().expect("load");
        for feeling in lexicon.feelings() {
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
    fn the_bias_keys_map_to_the_registry_parameters_they_name() {
        // The closed registry is what stops the lexicon and the loop drifting
        // apart, and nothing else guards the mapping: a parse that sent every
        // key to one parameter would leave every other test in this crate
        // passing.
        let lexicon = Lexicon::load_embedded().expect("load");

        let grit = lexicon.get("grit").expect("grit is in the lexicon");
        assert_eq!(
            grit.bias,
            BTreeMap::from([
                (CognitionParameter::Abandon, -0.25),
                (CognitionParameter::Persist, 0.4),
            ]),
            "grit gives up and carries on, so it biases abandon and persist"
        );

        let conscience = lexicon
            .get("conscience")
            .expect("conscience is in the lexicon");
        assert_eq!(
            conscience.bias,
            BTreeMap::from([
                (CognitionParameter::Refuse, 0.25),
                (CognitionParameter::Verify, 0.15),
            ]),
            "conscience is the only feeling in the lexicon that pushes refuse up"
        );

        let skepticism = lexicon
            .get("skepticism")
            .expect("skepticism is in the lexicon");
        assert_eq!(
            skepticism.bias,
            BTreeMap::from([(CognitionParameter::Verify, 0.55)]),
            "withholding assent is a verify bias"
        );
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
            matched.distance < EXACT_POINT_TOLERANCE,
            "the exact canonical point is at distance zero"
        );
        let runner_up_distance = matched
            .runner_up_distance
            .expect("a runner-up carries its own distance");
        assert!(
            runner_up_distance > matched.distance,
            "the runner-up is further away than the primary, got {runner_up_distance} and {}",
            matched.distance
        );
    }

    #[test]
    fn a_blend_lands_between_its_two_closest_labels() {
        let lexicon = Lexicon::load_embedded().expect("load");
        // Just inside glee (0.80, 0.65, -0.25), on the way to mirth
        // (0.75, 0.65, -0.25). These two name each other as nearest, and the
        // next label is more than 0.09 away, so the reading must resolve to
        // exactly this pair.
        let blend = Affect {
            valence: 0.78,
            arousal: 0.65,
            tension: -0.25,
        };
        let matched = lexicon.label(blend);
        let ids = [
            matched.primary.id.clone(),
            matched.runner_up.map(|r| r.id).unwrap_or_default(),
        ];
        assert!(
            ids.contains(&"glee".to_string()) && ids.contains(&"mirth".to_string()),
            "a blend must resolve to the two labels it sits between, got {ids:?}"
        );
        assert!(
            !ids.contains(&"joy".to_string()),
            "the third label is further away and must not take a slot, got {ids:?}"
        );
    }

    #[test]
    fn a_malformed_lexicon_is_a_yaml_error() {
        let error = Lexicon::load_sources(&["- id: broken\n  name: Broken\n"])
            .expect_err("a feeling without a family cannot be read");
        assert!(matches!(error, AffectError::Yaml(_)), "got {error:?}");
    }

    #[test]
    fn an_empty_lexicon_is_rejected() {
        let error =
            Lexicon::load_sources(&[]).expect_err("a lexicon with no feelings has no label");
        assert!(matches!(error, AffectError::Empty), "got {error:?}");
    }

    #[test]
    fn a_bias_key_outside_the_registry_is_rejected() {
        let error = Lexicon::load_sources(&[LEXICON_WITH_UNKNOWN_BIAS_KEY])
            .expect_err("a bias the loop does not read is a no-op");
        assert!(
            matches!(&error, AffectError::UnknownBiasParameter(id, key) if id == "rogue" && key == "not_a_parameter"),
            "got {error:?}"
        );
    }

    #[test]
    fn a_feeling_with_an_empty_bias_is_rejected() {
        let error = Lexicon::load_sources(&[LEXICON_WITH_EMPTY_BIAS])
            .expect_err("a feeling that cannot move behaviour is a label");
        assert!(
            matches!(&error, AffectError::EmptyBias(id) if id == "hollow"),
            "got {error:?}"
        );
    }

    #[test]
    fn a_canonical_point_outside_the_axis_range_is_rejected() {
        let error = Lexicon::load_sources(&[LEXICON_WITH_OUT_OF_RANGE_AXIS])
            .expect_err("a point outside the space cannot be labelled");
        assert!(
            matches!(&error, AffectError::AxisOutOfRange(id, axis, value, _) if id == "wild" && axis == "valence" && *value == 1.5),
            "got {error:?}"
        );
    }
}
