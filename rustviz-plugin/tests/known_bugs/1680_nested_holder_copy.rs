// Known bug, from the closed 2.x fix PRs (cx/oracle3-1677-1680-1683-1686); kept as a RustViz 3 regression case.
// Issue(s): #1680 (see known_bugs/README.md for what main draws wrong).
struct Holder<'a> { r: &'a String }
fn main() {
    let t = String::from("a");
    let g = Holder { r: &t };
    if t.is_empty() {
    } else if t.len() > 1 {
        let g2 = Holder { r: g.r };
        println!("{}", g2.r);
    } else {
    }
}
