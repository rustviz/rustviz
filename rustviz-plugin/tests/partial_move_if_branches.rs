// #395: fields moved out in one branch of an if/else.
// - `p`: `p.a` is moved in the `if` branch, `p.b` in the `else`; at
//   the end exactly one of them is still there and is dropped.
// - `q`: `q.a` is moved in one branch only, `q.b` on every path.
// - `r`: `r.a` is moved in both branches, so it's moved for sure.

struct P {
    a: String,
    b: String,
}

fn take(s: String) -> usize {
    s.len()
}

fn split(c: bool) {
    let p = P { a: String::from("a"), b: String::from("b") };
    if c {
        take(p.a);
    } else {
        take(p.b);
    }
}

fn one(c: bool) {
    let q = P { a: String::from("a"), b: String::from("b") };
    if c {
        take(q.a);
    } else {
        take(String::new());
    }
    take(q.b);
}

fn both(c: bool) {
    let r = P { a: String::from("a"), b: String::from("b") };
    if c {
        take(r.a);
    } else {
        take(r.a);
    }
}

fn main() {
    split(true);
    one(true);
    both(true);
}
