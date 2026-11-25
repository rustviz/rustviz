// A reference declared in a middle arm of a three-arm match that takes
// over a borrow from the caller (`let m2 = m;`, m a `&mut` parameter):
// its borrow ends in its own arm (#1502). It was drawn ending in the next
// arm, after m2 went out of scope. (Pinned by `corpus_invariants_hold`:
// event-on-dead-column on main.)
fn tuple_arms(m: &mut String, o: Option<String>, p: Option<String>) {
    match (&o, &p) {
        (None, _) => {}
        (Some(x), None) => {
            let m2 = m;
        }
        (Some(x), Some(y)) => {}
    }
}

fn slice_arms(m: &mut String) {
    let v = vec![String::from("ab"), String::from("ab")];
    match v.as_slice() {
        [] => {}
        [x] => {
            let m2 = m;
        }
        [x1, rest @ ..] => {}
    }
}

fn shared_ref(r: &String, n: u32) {
    match n {
        0 => {}
        1 => {
            let r2 = r;
        }
        _ => {}
    }
}

fn main() {}
