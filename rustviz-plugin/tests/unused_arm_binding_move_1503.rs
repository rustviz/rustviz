// A match-arm binding the arm never uses, over a borrowed scrutinee, in an
// arm that moves what it borrows (#1503). The binding isn't live anywhere,
// so its borrow ends where the pattern binds it: that's why the move
// compiles. It was drawn lasting to the arm's end, so o was moved while
// lent to x and then given back after the move. (Pinned by
// `corpus_invariants_hold`: main drew "no longer borrowed" on the moved
// column, event-on-dead-column.)
fn on_ref(o: Option<String>) {
    match &o {
        None => {}
        Some(x) => {
            drop(o);
        }
    }
}

// `v.as_slice()` and `&v[..]` scrutinees, slice patterns.
fn on_slice(v: Vec<String>) {
    match v.as_slice() {
        [] => {}
        [first, ..] => {
            drop(v);
        }
    }
}

fn on_index(v: Vec<String>) {
    match &v[..] {
        [] => {}
        [x] => {
            drop(v);
        }
        [x1, rest @ ..] => {}
    }
}

// A tuple of borrows.
fn on_tuple(o: Option<String>, p: Option<String>) {
    match (&o, &p) {
        (Some(x), _) => {
            drop(o);
        }
        _ => {}
    }
}

// One-line arms: the borrow ends before the move on the same line.
fn one_line_arm(o: Option<String>) {
    match &o {
        Some(x) => drop(o),
        None => {}
    }
}

fn lone_one_line_arm() {
    let arr = [String::from("a"), String::from("b")];
    match &arr {
        [x2, ..] => drop(arr),
    }
}

// The same order on a plain line: r's borrow is over before the move.
fn let_then_move() {
    let arr = [String::from("a"), String::from("b")];
    let r = &arr; drop(arr);
}

// A binding the arm does use keeps the borrow to that use.
fn used_then_moved(o: Option<String>) {
    match &o {
        Some(x) => {
            println!("{}", x);
            drop(o);
        }
        None => {}
    }
}

fn main() {}
