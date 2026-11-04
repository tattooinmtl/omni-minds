// Rust guideline compliant 2026-03-30

//! The per-event cost of appending to the log.
//!
//! The log is the only source of truth and every cognitive step appends to it,
//! so the append is the hottest path in the system. This benchmark measures it
//! against an in-memory log, so the number is the append path and not the
//! disk.
//!
//! Every event appended here is the same event: the same kind, the same
//! affect state, the same salience. The measurement is therefore of the append
//! and not of the payload, and every iteration does the same work.
//!
//! The throughput is one element per iteration, so the time criterion reports
//! is the cost of one event rather than the cost of a batch of them.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use si_log::{AffectSnapshot, EventKind, Log};

/// The label every appended event carries.
///
/// A fixed label keeps the payload the same size on every iteration: a label
/// that changed would change how much the append costs and the benchmark
/// would be measuring the payload rather than the append.
const LABEL: &str = "calm";

/// How the affect state moved, on the valence, arousal and tension axes.
///
/// A small movement in each direction. The values are arbitrary but finite,
/// which is what the append path expects: a number that is not finite is
/// repaired before the row is written, and the repair is a separate path that
/// this benchmark deliberately does not measure.
const DELTA: [f32; 3] = [0.1, 0.0, -0.2];

/// The affect state after the movement.
///
/// The same as the neutral state moved by [`DELTA`]. It is finite, so the
/// append writes it as it stands.
const AFTER: [f32; 3] = [0.1, 0.0, -0.2];

/// The affect state every appended event carries.
///
/// The snapshot is the one the mind holds at the moment it thinks, so an
/// append without it is not an append the mind ever makes.
const SNAPSHOT: AffectSnapshot = AffectSnapshot {
    valence: 0.1,
    arousal: 0.0,
    tension: -0.2,
};

/// How much the appended event pulls the mind's attention.
///
/// Any finite number in `[0, 1]` would do. It is fixed so that every
/// iteration writes the same column.
const SALIENCE: f32 = 0.5;

/// The event every iteration appends.
///
/// Built inside the measured loop because the caller builds it: the cost of
/// constructing the payload is part of what an append costs, and hiding it
/// behind a clone outside the loop would measure a path the mind does not
/// take.
fn felt() -> EventKind {
    EventKind::Felt {
        delta: DELTA,
        after: AFTER,
        label: LABEL.to_string(),
    }
}

/// Measures the append path.
fn benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("append");
    // One event per iteration, so the reported time is the cost of one event.
    group.throughput(Throughput::Elements(1));
    group.bench_function("felt", |b| {
        // The log is opened once, outside the measured function, so the
        // measurement is the append and not the schema.
        let log = Log::open_in_memory().expect("an in-memory log opens");
        b.iter(|| {
            let event = log
                .append(felt(), None, Some(SNAPSHOT), SALIENCE)
                .expect("the event appends");
            black_box(&event);
        });
    });
    group.finish();
}

criterion_group!(append, benches);
criterion_main!(append);
