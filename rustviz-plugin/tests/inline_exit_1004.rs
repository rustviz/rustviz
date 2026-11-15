// A body that always runs and always returns, drawn inline (an `if let`
// whose pattern always matches, a one-arm `match`), leaves the function
// there (#1004): what it drops is dropped on the `return` line, and the
// code after it never runs, so nothing is drawn for it.
#![allow(irrefutable_let_patterns)]

// x (f's String) and g are dropped at the `return`; `g.len()` never runs.
fn early(f: String, g: String) -> usize {
    if let x = f {
        return x.len();
    }
    g.len()
}

// The same with a one-arm `match`.
#[allow(unreachable_code)]
fn single(p: String, q: String) -> usize {
    match p {
        y => {
            return y.len() + 1;
        }
    }
    q.len()
}

// Both branches return (#515): nothing after the `if` runs, so a isn't
// moved into z. It is dropped at each `return`.
#[allow(unreachable_code)]
fn both(c: bool, a: String, b: String) -> usize {
    if c {
        return 1;
    } else {
        return 2;
    }
    let z = a;
    z.len() + b.len()
}
