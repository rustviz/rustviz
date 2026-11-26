// #1585: an unused `ref` binding of a single-arm `match` (its borrow ends
// where the pattern binds it) followed by a nested `if .. { return; }`.
// The binding's end was listed after the nested conditional, so the
// lender was drawn still shared on both of its legs (its trunk ran
// through the fork), and in the struct form `p.a` was mutably lent while
// still immutably borrowed (`State::Invalid`). The binding is no longer
// drawn in the nested `if` after its loan ended.
struct Pair { a: String, b: String }

fn in_if_let(s: String, o: Option<String>, c: bool) {
    if let Some(x3) = o {
        match s {
            ref a => {
                if c { return; }
            }
        }
    }
}

fn field(mut p: Pair, c: bool) {
    if c {
        match p {
            Pair { a: ref a2, .. } => {
                let m2 = &mut p.a;
                drop(p.b);
            }
        }
    }
}

fn ref_mut(mut t: String, o2: Option<String>, c: bool) {
    if let Some(x4) = o2 {
        match t {
            ref mut a3 => {
                if c {
                    return;
                }
            }
        }
    }
}

// An unused or-pattern binding: its possible borrows end where the
// pattern binds it, so it isn't drawn in the nested `if` either (its join
// said "a8 still has no value here").
fn or_pattern(t8: (String, String, bool), c: bool) {
    match t8 {
        (ref a8, _, true) | (_, ref a8, false) => {
            if c { return; }
        }
    }
}
