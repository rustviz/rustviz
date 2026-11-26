// Known bug, from the closed 2.x fix PRs (cx/cx-c09); kept as a RustViz 3 regression case.
// Issue(s): #1710 (see known_bugs/README.md for what main draws wrong).
fn f(s: String, t: String, a: bool, c: bool) {
    let mut r;
    if a {
        r = &s;
        println!("{}", r);
    }
    if !a {
        if c {
            r = &t;
            println!("{}", r);
        } else {
        }
    }
}

fn main() {}
