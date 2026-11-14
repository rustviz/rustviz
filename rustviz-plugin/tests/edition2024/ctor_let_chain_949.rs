// A constructor scrutinee in a let chain is no actor either (#949, see
// #807 / #873): `Some(f)` builds a temporary `Option` holding f's
// String, and `Some(x)` moves it on to x, so f goes to x. `Some(x)` on
// `Some(f)` can't fail to match: f's String is in x before any part of
// the condition can turn out false, and is dropped with x when one does.
// `Some(Some(w))` on `Some(o)` can fail: o is moved into the temporary
// either way, and dropped with it when the pattern doesn't match.
fn chain(f: String) {
    if let Some(x) = Some(f) && x.len() > 2 {
        println!("{}", x);
    }
}

fn chain_else(h: String) {
    if let Some(y) = Some(h) && y.len() > 2 {
        println!("{}", y);
    } else {
        println!("short");
    }
}

fn chain_later(k: String, c: bool) {
    if c && let Some(z) = Some(k) {
        println!("{}", z);
    } else {
        println!("no");
    }
}

fn chain_nested(o: Option<String>) {
    if let Some(Some(w)) = Some(o) && w.len() > 2 {
        println!("{}", w);
    }
}

fn main() {
    chain(String::from("abc"));
    chain_else(String::from("ab"));
    chain_later(String::from("k"), true);
    chain_nested(Some(String::from("abc")));
}
