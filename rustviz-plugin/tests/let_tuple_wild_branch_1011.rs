// `let (a, _) = if c { (f, g) } else { (h, k) };` (#1011): each branch
// builds a tuple. a takes its first String; the `_` leaves the second in
// the tuple, a temporary that Rust drops at the end of the `let`
// statement, on the path through that branch. The other branch's Strings
// are dropped at the end of the function (a runtime drop flag).
fn wild(c: bool, f: String, g: String, h: String, k: String) {
    let (a, _) = if c { (f, g) } else { (h, k) };
    drop(a);
}

// The same with a `match`, over several lines.
fn wild_match(n: u8, p: String, q: String, r: String, s: String) {
    let (_, b) = match n {
        0 => (p, q),
        _ => (r, s),
    };
    drop(b);
}
