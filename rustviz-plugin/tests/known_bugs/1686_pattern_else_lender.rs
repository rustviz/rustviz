// Known bug, from the closed 2.x fix PRs (cx/oracle3-1677-1680-1683-1686); kept as a RustViz 3 regression case.
// Issue(s): #1686 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let mut v = vec![String::new()];
    let s = String::new();
    if let Some(x) = v.first() {
        let r = v.last().unwrap();
        drop(s);
    } else {
        'blk: {
            let n = 1;
            v[0] = s;
            if v.len() > 1 { break 'blk; }
        }
    }
}
