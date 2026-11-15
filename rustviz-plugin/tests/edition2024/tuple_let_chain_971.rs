// A tuple literal in a let chain is taken apart like a lone `if let (x,
// _) = (f, s)` (#971, see #327): `(f, s)` moves both Strings into the
// scrutinee's temporary tuple before any part of the condition can turn
// out false, so f's String goes to x once, and the tuple is no actor.
// s's String stays in the tuple, which is dropped at the end of the if
// branch, or before the code after the `if` (or the `else`) when the
// condition fails (edition 2024); x is dropped then too. Neither f nor
// s owns anything after the condition: no drop flag.
fn tuple_chain(f: String, s: String) {
    if let (x, _) = (f, s) && x.len() > 2 {
        println!("{}", x);
    }
}

fn tuple_chain_else(g: String, t: String) {
    if let (y, _) = (g, t) && y.len() > 2 {
        println!("{}", y);
    } else {
        println!("short");
    }
}

// The tuple is built only when c holds: k and m may still be owned
// after the `if` (a drop flag).
fn tuple_chain_later(k: String, m: String, c: bool) {
    if c && let (z, _) = (k, m) {
        println!("{}", z);
    }
}

// A `return` in the body drops the tuple there.
fn tuple_chain_exit(a: String, b: String) -> usize {
    if let (p, _) = (a, b) && p.len() > 2 {
        return p.len();
    }
    0
}

// A pattern that can fail: o's value is dropped with the tuple if it
// doesn't match.
fn tuple_chain_refutable(o: Option<String>, u: String) {
    if let (Some(w), _) = (o, u) && w.len() > 2 {
        println!("{}", w);
    }
}

fn main() {
    tuple_chain(String::from("abc"), String::from("s"));
    tuple_chain_else(String::from("ab"), String::from("t"));
    tuple_chain_later(String::from("k"), String::from("m"), true);
    tuple_chain_exit(String::from("abc"), String::from("b"));
    tuple_chain_refutable(Some(String::from("abc")), String::from("u"));
}
