fn consume(s: String) -> usize { s.len() }

// Every `else` path returns, so only the `if c` path reaches `0`, and
// on it `s` was moved: nothing is dropped at the end of `f` (#491).
fn f(c: bool) -> usize {
    let s = String::from("s");
    if c {
        consume(s);
    } else if s.len() > 3 {
        return 1;
    } else {
        return 2;
    }
    0
}

// A `match` as the else branch whose arms all return diverges too.
fn g(c: bool, n: i32) -> usize {
    let s = String::from("s");
    if c {
        consume(s);
    } else {
        match n {
            0 => return 1,
            _ => return 2,
        }
    }
    0
}

fn main() {
    f(true);
    g(true, 0);
}
