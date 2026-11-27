// A lone `if let` whose pattern always matches (here a plain binding,
// `if let x = f`, which rustc warns about as `irrefutable_let_patterns`)
// always runs its body (#995): it's drawn inline, like a plain block,
// even when the body always leaves (`return`, `break`) or assigns a
// `let y;` local. There's no path that skips the body, so nothing is
// "maybe moved" and nothing is dropped with a runtime drop flag.
#![allow(irrefutable_let_patterns)]

// f's String goes to x, which `return` drops, with g; the code after the
// `if` never runs, so nothing is drawn for it (#1004).
fn early(f: String, g: String) -> usize {
    if let x = f {
        return x.len();
    }
    g.len()
}

// a and s1 are always moved, and y always holds s1's String: it's dropped
// when y goes out of scope, with no drop flag. (The borrow checker still
// treats y as maybe unassigned after the `if`: using it there is E0381.)
fn noelse_assign(a: String, s1: String) {
    let y;
    if let z = a {
        drop(z);
        y = s1;
    }
}

// A dead `else` has its own leg (#1873). Like `early`, only the return runs.
fn dead_else_early(p: String, r: String) -> usize {
    if let w = p {
        return w.len();
    } else {
        drop(r);
    }
    0
}

// A body that always `break`s: s's String always goes to t.
fn in_loop(v: Vec<String>) {
    for s in v {
        if let t = s {
            drop(t);
            break;
        }
    }
}
