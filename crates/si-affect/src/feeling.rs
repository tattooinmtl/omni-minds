// Rust guideline compliant 2026-03-30

//! One feeling: a canonical point in affect space with a definition and a bias.

use crate::affect::Affect;
use crate::bias::CognitionParameter;
use std::collections::BTreeMap;

/// A labelled region of the affect space.
#[derive(Debug, Clone, PartialEq)]
pub struct Feeling {
    pub id: String,
    pub name: String,
    pub family: String,
    pub definition: String,
    pub valence: f32,
    pub arousal: f32,
    pub tension: f32,
    /// How this feeling moves behaviour.
    pub bias: BTreeMap<CognitionParameter, f32>,
    /// The feelings whose canonical points are closest to this one.
    ///
    /// The ids of the three other feelings nearest in the three-axis affect
    /// space. The comparison is exact: every coordinate is a hundredth, so the
    /// squared distance scaled by 10000 — that is, `(dx² + dy² + dz²) × 10000`
    /// — is an exact integer, and the rule compares those integers. Ties are
    /// broken by the position of the feeling in the embedded lexicon — file
    /// order, then line order — and the field is derived from the coordinates
    /// rather than written by hand: `tools/regenerate_nearest.py` writes it,
    /// and `tools/validate_lexicon.py` refuses a lexicon whose lists disagree
    /// with that rule.
    pub nearest: Vec<String>,
}

impl Feeling {
    /// The canonical affect position of this feeling.
    pub fn point(&self) -> Affect {
        Affect {
            valence: self.valence,
            arousal: self.arousal,
            tension: self.tension,
        }
    }
}
