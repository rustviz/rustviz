// A lone `if let` whose pattern can't fail on the literal it takes apart
// (#981): `(x, _)` matches every `(f, s)` (rustc warns
// `irrefutable_let_patterns`), and `Some(x)` matches every `Some(f)`. The
// body always runs, so there is no path where "the pattern didn't match":
// f's String goes to x and is dropped with it at the end of the body, and
// s's String is dropped with the tuple there (edition 2024).
fn take(_g: String) {}

fn tuple_lit(f: String, s: String) {
    if let (x, _) = (f, s) {
        println!("{}", x);
    }
}

fn ctor_lit(a: String) {
    if let Some(y) = Some(a) {
        println!("{}", y);
    }
}

// The body always runs, so g is surely moved: no drop flag.
fn moves_outer(b: String, c: String, g: String) {
    if let (z, _) = (b, c) {
        println!("{}", z);
        take(g);
    }
}

// An `else` here is dead code: it never runs.
fn tuple_else(h: String, k: String) {
    if let (w, _) = (h, k) {
        println!("{}", w);
    } else {
        println!("never");
    }
}

fn ctor_else(m: String) {
    if let Some(v) = Some(m) {
        println!("{}", v);
    } else {
        println!("never");
    }
}

// A body that always returns is drawn as a branch; the path that skips
// it never runs.
fn early(p: String, q: String) -> usize {
    if let (r, _) = (p, q) {
        return r.len();
    }
    0
}

// Control: `Some(t)` can fail on o's Option, so the tuple is dropped
// before the code after the `if` when it doesn't match.
fn refutable(o: Option<String>, u: String) {
    if let (Some(t), _) = (o, u) {
        println!("{}", t);
    }
}

// Control: `Some(Some(n))` can fail on `Some(e)` (e may be None).
fn refutable_nested(e: Option<String>) {
    if let Some(Some(n)) = Some(e) {
        println!("{}", n);
    }
}
