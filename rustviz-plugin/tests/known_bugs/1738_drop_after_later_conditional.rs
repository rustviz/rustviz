// Known bug, from the closed 2.x fix PRs (cx/cx-c09); kept as a RustViz 3 regression case.
// Issue(s): #1738 (see known_bugs/README.md for what main draws wrong).
fn no_move(c: bool, d: bool, stop: bool) {
    let mut z;
    if c {
        z = String::new();
    }
    if d {
        z = String::new();
    }
    if stop { return; }
    z = String::from("last");
    println!("{}", z);
}

fn with_try(c: bool, d: bool, o: Option<()>) -> Option<()> {
    let mut x;
    if c {
        x = String::new();
    }
    if d {
        x = String::new();
    }
    o?;
    x = String::from("last");
    println!("{}", x);
    Some(())
}

fn main() {}
