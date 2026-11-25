// #1583: a struct whose last fields are moved in some arms of a
// conditional only (an `if` / `else if` chain with no final `else`, or an
// `else` that moves nothing): on the paths that run neither arm it still
// holds them, so its line goes on to its scope end, where they are
// dropped if still there. Main stopped its line at the join.
struct Pair {
    a: String,
    b: String,
}

fn else_if(p: Pair, c: bool, d: bool) {
    let s2 = p.b;
    if c {
        drop(p.a);
    } else if d {
        drop(p.a);
    }
}

fn nested_in_else(q: Pair, c: bool, d: bool) {
    let s3 = q.b;
    if c {
        drop(q.a);
    } else {
        if d {
            drop(q.a);
        }
    }
}

fn empty_else(w: Pair, c: bool, d: bool) {
    let s4 = w.b;
    if c {
        drop(w.a);
    } else if d {
        drop(w.a);
    } else {
    }
}

fn maybe_moved_before(x: Pair, c: bool, d: bool, e: bool) {
    if e {
        drop(x.a);
    }
    if c {
        drop(x.b);
    } else if d {
        drop(x.b);
    }
}

fn destructured(y: Pair, c: bool, d: bool) {
    if c {
        let Pair { a, b } = y;
    } else if d {
        let Pair { a, b } = y;
    }
}

fn one_arm(z: Pair, c: bool) {
    let s5 = z.b;
    if c {
        drop(z.a);
    }
}

fn both_arms(u: Pair, c: bool) {
    let s6 = u.b;
    if c {
        drop(u.a);
    } else {
        drop(u.a);
    }
}

fn mk() -> Pair {
    Pair { a: String::from("a"), b: String::from("b") }
}

fn main() {
    else_if(mk(), true, false);
    nested_in_else(mk(), true, false);
    empty_else(mk(), true, false);
    maybe_moved_before(mk(), true, false, true);
    destructured(mk(), true, false);
    one_arm(mk(), true);
    both_arms(mk(), true);
}
