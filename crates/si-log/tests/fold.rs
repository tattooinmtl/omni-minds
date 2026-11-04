use si_log::{EventKind, Log, Project};

#[derive(Default, Debug)]
struct CountThought {
    thoughts: u32,
    answers: u32,
}

impl Project for CountThought {
    type State = (u32, u32);
    fn apply(&mut self, event: &si_log::Event) {
        match &event.kind {
            EventKind::Thought { .. } => self.thoughts += 1,
            EventKind::Responded { .. } => self.answers += 1,
            _ => {}
        }
    }
    fn finish(self) -> Self::State {
        (self.thoughts, self.answers)
    }
}

#[test]
fn folding_to_a_sequence_ignores_later_events() {
    let log = Log::open_in_memory().expect("open");
    log.append(
        EventKind::Thought {
            text_ref: "t1".into(),
            confidence: 0.9,
        },
        None,
        None,
        1.0,
    )
    .expect("append");
    log.append(
        EventKind::Responded {
            answer_ref: "a1".into(),
        },
        None,
        None,
        0.5,
    )
    .expect("append");
    log.append(
        EventKind::Thought {
            text_ref: "t2".into(),
            confidence: 0.9,
        },
        None,
        None,
        1.0,
    )
    .expect("append");

    let (thoughts, answers) = log.fold_to(2, CountThought::default()).expect("fold");
    assert_eq!((thoughts, answers), (1, 1));
}

#[test]
fn folding_past_the_end_is_the_same_as_folding_to_the_end() {
    let log = Log::open_in_memory().expect("open");
    log.append(
        EventKind::Responded {
            answer_ref: "a1".into(),
        },
        None,
        None,
        0.0,
    )
    .expect("append");
    let last = log.last_seq().expect("last_seq");
    let a = log.fold_to(last, CountThought::default()).expect("fold");
    let b = log
        .fold_to(u64::MAX, CountThought::default())
        .expect("fold");
    assert_eq!(a, b);
}
