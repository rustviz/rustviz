// Known bug, from the closed 2.x fix PRs (cx/cx-c04); kept as a RustViz 3 regression case.
// Issue(s): #1720 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let n = m.len();
        let r3 = &mut s2;
        let m2 = &mut s2;
    } else { m.push('a'); }
}
