// Known bug, from the closed 2.x fix PRs (cx/cx-c09); kept as a RustViz 3 regression case.
// Issue(s): #1753 (see known_bugs/README.md for what main draws wrong).
fn f(s: String, t: String, u: String, a: bool, b: bool) {
    let r;
    if a {
        r = &s;
    } else {
        r = &t;
    }
    println!("{}", r);
    let mut q = r;
    if b {
    } else {
        if b {
            q = &u;
            println!("{}", q);
        } else {
        }
    }
}

fn main() {}
