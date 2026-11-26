// Known bug, from the closed 2.x fix PRs (cx/oracle3-1677-1680-1683-1686); kept as a RustViz 3 regression case.
// Issue(s): #1686 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let v = vec![String::new()];
    let s = String::new();
    if let Some(_) = v.first() && v.len() > 1 && let Some(_) = v.last() {
        if let Some(x2) = v.first() && v.len() > 1 && let Some(y2) = v.last() {
            let c = move || s;
            let n = 1;
        }
    } else {
        let n = v.len();
    }
}
