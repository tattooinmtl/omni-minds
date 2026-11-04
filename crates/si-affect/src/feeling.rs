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
    /// The closest feelings, by id.
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
