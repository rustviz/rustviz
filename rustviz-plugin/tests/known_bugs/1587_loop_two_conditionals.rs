// Known bug, from the closed 2.x fix PRs (cx/cx-c09); kept as a RustViz 3 regression case.
// Issue(s): #1587 (see known_bugs/README.md for what main draws wrong).
fn run(c: bool, d: bool) {
    let mut s = String::from("a");
    drop(s);
    for i in 0..2 {
        s = String::from("b");
    }
    if c {
        s = String::from("b");
    }
    if d {
        s = String::from("c");
    } else {
    }
}

// A caller-supplied range can actually execute zero times.
fn variable_length(count: usize, first: bool, next: Option<()>) {
    let mut t = String::new();
    drop(t);
    for _ in 0..count {
        t = String::from("loop");
    }
    if first {
        t = String::from("first");
    }
    match next {
        Some(()) => { t = String::from("last"); }
        None => {}
    }
}

fn main() {}
