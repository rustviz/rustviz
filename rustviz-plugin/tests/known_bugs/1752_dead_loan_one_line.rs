// Known bug, from the closed 2.x fix PRs (cx/cx-text-b); kept as a RustViz 3 regression case.
// Issue(s): #1752 (see known_bugs/README.md for what main draws wrong).
fn run(mut s: String, t: String) {
    let mut r = &s;
    let n = r.len(); r = &t; s.push('x'); r = &s;
    println!("{}", r);
}
