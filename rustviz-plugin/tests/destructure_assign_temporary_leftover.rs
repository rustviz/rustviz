// #625: a destructuring assignment whose pattern leaves part of a call's
// result (or of a field of it) in the temporary. rustc lowers it to
// `{ let (lhs, _) = pair(); m = lhs; }`, so the rest is dropped at the end
// of that `let`: before m gets its new value (checked with a
// Drop-printing program).
struct W {
    pair: (String, String),
    other: String,
}

fn pair() -> (String, String) {
    (String::from("a"), String::from("b"))
}

fn make_w() -> W {
    W { pair: (String::from("a"), String::from("b")), other: String::from("o") }
}

fn main() {
    let mut m = String::from("m");
    let mut n = String::from("n");
    (m, _) = pair();
    (n, _) = make_w().pair;
    // Takes all of it: no column.
    (m, n) = pair();
    println!("{} {}", m, n);
}
