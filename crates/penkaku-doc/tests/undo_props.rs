//! Undo/redo invert every command: random edit sessions, then undo everything (back to the
//! original) and redo everything (back to the final state). Ids stay unique throughout.

use penkaku_doc::{Align, EditDoc};
use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Op {
    Insert(String),
    Backspace(bool),
    Delete(bool),
    Move(usize, bool),
    Select(usize, usize),
    Colour(u8),
    Align(u8),
    Ime(String, String),
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    let text = proptest::collection::vec(prop_oneof![Just('a'), Just('好'), Just(' '), Just('\n'), Just('😀')], 0..4)
        .prop_map(|v| v.into_iter().collect::<String>());
    prop_oneof![
        3 => text.clone().prop_map(Op::Insert),
        1 => any::<bool>().prop_map(Op::Backspace),
        1 => any::<bool>().prop_map(Op::Delete),
        1 => (0usize..40, any::<bool>()).prop_map(|(i, e)| Op::Move(i, e)),
        1 => (0usize..40, 0usize..40).prop_map(|(a, b)| Op::Select(a, b)),
        1 => any::<u8>().prop_map(Op::Colour),
        1 => (0u8..4).prop_map(Op::Align),
        1 => (text.clone(), text).prop_map(|(a, b)| Op::Ime(a, b)),
        1 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

fn run(d: &mut EditDoc, op: &Op) {
    match op {
        Op::Insert(t) => d.insert(t),
        Op::Backspace(w) => {
            d.backspace(*w);
        }
        Op::Delete(w) => {
            d.delete(*w);
        }
        Op::Move(i, e) => d.move_to(*i, *e),
        Op::Select(a, b) => d.select_range(*a.min(b)..*a.max(b)),
        Op::Colour(c) => d.set_attrs(|a| a.colour = Some([*c, 0, 0])),
        Op::Align(k) => d.set_align([Align::Left, Align::Center, Align::Right, Align::Justify][*k as usize]),
        Op::Ime(a, b) => {
            d.set_marked(None, a, None);
            d.commit_text(None, b);
        }
        Op::Undo => {
            d.undo();
        }
        Op::Redo => {
            d.redo();
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn undo_all_restores_and_redo_all_replays(start in "[a好\n ]{0,12}", ops in proptest::collection::vec(op(), 0..40)) {
        let mut d = EditDoc::from_text(&start);
        let original = d.document().clone();
        for o in &ops {
            run(&mut d, o);
            prop_assert!(d.document().is_consistent(), "after {:?}", o);
        }
        // Steps undone in the session can still be redone: the furthest state is after them.
        while d.redo() {}
        let last = d.document().clone();
        while d.undo() {}
        prop_assert!(d.document().same_content(&original));
        while d.redo() {}
        prop_assert!(d.document().same_content(&last));
        prop_assert!(d.document().is_consistent());
    }
}
