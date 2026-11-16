// A body that always runs and always returns, inside a branch (an `if let`
// whose pattern always matches, a one-arm `match`, a plain block), and a
// plain block that returns directly in the function's body (#1013): what
// the `return` drops is dropped on its line, and the code after it never
// runs, so nothing is drawn for it.
#![allow(irrefutable_let_patterns, unreachable_code)]

// x (f's String) and g are dropped at the `return`; `g.len()` never runs.
fn nested(c: bool, f: String, g: String) -> usize {
    if c {
        if let x = f {
            return x.len();
        }
        g.len()
    } else {
        0
    }
}

// The same with a one-arm `match`: y (p's String) and q.
fn nested_arm(c: bool, p: String, q: String) -> usize {
    if c {
        match p {
            y => {
                return y.len();
            }
        }
        q.len()
    } else {
        0
    }
}

// A plain block in a branch: z (a's String) and b are dropped at the
// `return`, not z again where the block ends.
fn block_in_branch(c: bool, a: String, b: String) -> usize {
    if c {
        {
            let z = a;
            return z.len();
        }
        b.len()
    } else {
        b.len()
    }
}

// A plain block directly in the function's body: w (d's String) and e are
// dropped at the `return`; `e.len()` never runs.
fn block(d: String, e: String) -> usize {
    {
        let w = d;
        return w.len();
    }
    e.len()
}

// In a loop body: `println!` after the `if let` never runs. (Known gap,
// #1020: in a loop body the `return`'s drops of t, the iterator and h are
// drawn at their scope ends.)
fn in_loop(v: Vec<String>, h: String) -> usize {
    for s in v {
        if let t = s {
            return t.len();
        }
        println!("{}", h);
    }
    0
}
