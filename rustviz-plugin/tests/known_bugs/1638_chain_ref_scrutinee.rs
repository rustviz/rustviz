// Known bug, from the closed 2.x fix PRs (cx/hedge-f2); kept as a RustViz 3 regression case.
// Issue(s): #1638 (see known_bugs/README.md for what main draws wrong).
#![allow(irrefutable_let_patterns)]
fn chain(check: bool) {
    let mut value = 0;
    let mut reference = &mut value;
    if let link = &mut reference && check {
        **link += 1;
    }
    *reference += 1;
}

fn main() { chain(true); }
