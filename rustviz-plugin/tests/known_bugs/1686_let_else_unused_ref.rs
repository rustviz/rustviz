// Known bug, from the closed 2.x fix PRs (cx/oracle3-1677-1680-1683-1686); kept as a RustViz 3 regression case.
// Issue(s): #1686 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let v = vec![String::new()];
    let Some(x) = v.first() else {
        let r = &v[0];
        return;
    };
}
