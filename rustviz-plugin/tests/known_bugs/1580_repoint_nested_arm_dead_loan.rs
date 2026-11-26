// Known bug, from the closed 2.x fix PRs (fix/r3-repoint); kept as a RustViz 3 regression case.
// Issue(s): #1580 (see known_bugs/README.md for what main draws wrong).
// A reference whose old loan is dead before an `if` (never read again)
// re-pointed in an arm of a conditional nested in one of its arms: an
// `else if` chain, an `else { if .. }`, a `match` in an `if let` arm. Both
// joins say the borrow ended before the branches above, and each borrow a
// branch gave r ended in that branch (#1580). The inner join used to say
// "r still has no value here: none of the branches above assigned it",
// and the outer one had no dot.

fn else_if_chain(s: String, t: String, flag: bool) {
    let mut r = &s;
    if flag {
    } else if t.len() > 1 {
        r = &t;
    } else {
    }
}

fn final_else(s: String, t: String, flag: bool) {
    let mut r = &s;
    if flag {
    } else if t.len() > 1 {
    } else {
        r = &t;
    }
}

fn else_block_if(s: String, t: String, flag: bool) {
    let mut r = &s;
    if flag {
    } else {
        if t.len() > 1 {
            r = &t;
        } else {
        }
    }
}

fn match_in_if_let(s: String, t: String, o: Option<i32>) {
    let mut r = &s;
    if let Some(n) = o {
        match n {
            1 => {
                r = &t;
            }
            _ => {}
        }
    } else {
    }
}

// Last read in the condition of an `if` nested in an arm: the old loan
// ends there, and on the outer `else` path as that arm starts.
fn read_in_nested_condition(s: String, t: String, a: bool, b: bool) {
    let mut r = &s;
    if a {
        if r.is_empty() {
        } else if b {
        } else {
            r = &t;
        }
    } else {
    }
}

fn main() {}
