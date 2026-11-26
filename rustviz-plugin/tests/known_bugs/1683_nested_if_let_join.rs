// Known bug, from the closed 2.x fix PRs (cx/oracle3-1677-1680-1683-1686); kept as a RustViz 3 regression case.
// Issue(s): #1683 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let s = String::from("s");
    let v = vec![String::from("a"), String::from("b")];
    if let Some(x) = v.first() {
        if let Some(_y) = v.last() {
            let _t = s;
        }
        if x.is_empty() {
            println!("{}", x);
        }
    }
    println!("{}", v.len());
}
