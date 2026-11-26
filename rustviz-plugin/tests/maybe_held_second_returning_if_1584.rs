// #1584: s may hold a value only if c was true. In the `if d` arm, the
// second `if` (whose arm declares t before returning) splits s's column
// at a dot on its own line, like the first `if`. Main drew no dot there:
// the `then` leg started in mid-air, and the split's text went onto the
// `return`'s drop dot and the join's.
fn run(c: bool, d: bool, e: bool, f: bool) {
    let mut s = String::from("ab");
    drop(s);
    if c {
        s = String::from("ab");
    }
    if d {
        if e {
            return;
        }
        if f {
            let _t = String::from("x");
            return;
        }
    } else {
    }
}

fn in_match(c: bool, d: bool, e: bool, k: u8) {
    let mut m = String::from("ab");
    drop(m);
    if c {
        m = String::from("ab");
    }
    if d {
        if e {
            return;
        }
        match k {
            0 => {
                let _t = String::from("x");
                return;
            }
            _ => {}
        }
    } else {
    }
}
