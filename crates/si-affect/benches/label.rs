// Rust guideline compliant 2026-03-30

//! The per-call cost of labelling an affect state.
//!
//! The label is the nearest-neighbour query over the 250 feelings: the mind
//! reads its own state as a feeling, and that read happens on the path every
//! cognitive step takes. This benchmark measures it so the next change to
//! the query is measured rather than guessed.
//!
//! The lexicon is loaded once, outside the measured function. Loading it is
//! not the query, and a benchmark that reloads it every iteration measures
//! `serde_yaml` instead.
//!
//! Two points are measured: one exactly on a feeling's canonical point, and
//! one between two feelings. The first is the cheapest case the query has,
//! and the second is the typical one, where the state is near two feelings
//! and the label is a judgement between them.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use si_affect::{Affect, Lexicon};

/// Measures the nearest-neighbour query.
fn benches(c: &mut Criterion) {
    let lexicon = Lexicon::load_embedded().expect("the embedded lexicon loads");

    // The cheapest case: a state that sits exactly on a canonical point.
    let feelings = lexicon.feelings();
    let first = feelings.first().expect("the lexicon holds feelings");
    let canonical = first.point();

    // The typical case: a state between two feelings. The two are the first
    // feeling of the embedded lexicon and the one farthest from it, so the
    // point between them is as far from any canonical point as the space
    // allows. Both are read from the lexicon rather than written down here,
    // so the points follow the data when the data is regenerated.
    let farthest = feelings
        .iter()
        .max_by(|a, b| {
            canonical
                .squared_distance(a.point())
                .total_cmp(&canonical.squared_distance(b.point()))
        })
        .expect("the lexicon holds feelings");
    let other = farthest.point();
    let between = Affect {
        valence: (canonical.valence + other.valence) / 2.0,
        arousal: (canonical.arousal + other.arousal) / 2.0,
        tension: (canonical.tension + other.tension) / 2.0,
    };

    let mut group = c.benchmark_group("label");
    group.bench_function("on_canonical_point", |b| {
        b.iter(|| black_box(lexicon.label(black_box(canonical))));
    });
    group.bench_function("between_two_feelings", |b| {
        b.iter(|| black_box(lexicon.label(black_box(between))));
    });
    group.finish();
}

criterion_group!(label, benches);
criterion_main!(label);
