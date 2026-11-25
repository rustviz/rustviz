// Drawing conventions decided for #1560, #1489 and #1542 (2026-11-25).
// #1560: an arrow into (or out of) an arm's dot that can only come in along
// the arm's solid fork leg gets a row of its own under the arm's first line,
// and a level track below the fork.

// The return of `x` (unused) into `o`'s `if let` dot.
fn if_let_return(mut o: Option<String>, mut v: Vec<String>) -> Option<usize> {
    let s2 = v.remove(0);
    if let Some(x) = o.as_ref() {
        if let Some(x2) = v.last() { o = Some(s2); } else { }
    }
    Some(0)
}

// The same, `x2` bound in an `if let` after a `let else`.
fn maybe(s: String) -> Option<String> { Some(s) }
fn let_else_then_if_let() {
    let o = Some(String::from("ab"));
    let Some(mut x) = maybe(String::from("ab")) else { return; };
    if let Some(x2) = o.as_ref() {
        x = String::from("x");
    } else {
        if x.len() > 1 { return; }
    }
}

// The same in a `match` arm's `if let`.
fn match_then_if_let(o: Option<String>, k: u8) -> usize {
    match k {
        0 => {
            if let Some(x) = &o {
                return x.len();
            }
            0
        }
        _ => 1,
    }
}
