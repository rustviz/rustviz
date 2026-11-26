// Follow-ups to #1599 (Codex review): a struct none of whose fields can
// be used, some moved on some paths only. A nested struct is named by
// the fields moved out of it (z.pair was never moved as a whole), and a
// `mut` struct's fields can be assigned again. (second_conditional: p's
// line through the second `if` is #1595's.)
struct Pair {
    a: String,
    b: String,
}

struct Z {
    t: String,
    pair: Pair,
}

fn children(z: Z, c: bool) {
    drop(z.t);
    if c {
        drop(z.pair.a);
    } else {
        drop(z.pair.b);
    }
}

fn second_conditional(p: Pair, c: bool, d: bool) {
    if c {
        drop(p.a);
    }
    if d {
        drop(p.b);
    } else {
        drop(p.b);
    }
}

fn reassigned(mut m: Pair, c: bool, d: bool) {
    let s2 = m.b;
    if c {
        drop(m.a);
    } else if d {
        drop(m.a);
    }
    m.a = String::from("new");
    println!("{}", m.a);
}

fn children_one_by_one(y: Z, c: bool) {
    drop(y.pair.a);
    drop(y.pair.b);
    if c {
        drop(y.t);
    }
}

fn mk() -> Pair {
    Pair { a: String::from("a"), b: String::from("b") }
}

fn main() {
    children(Z { t: String::from("t"), pair: mk() }, true);
    second_conditional(mk(), true, false);
    reassigned(mk(), true, false);
    children_one_by_one(Z { t: String::from("t"), pair: mk() }, true);
}
