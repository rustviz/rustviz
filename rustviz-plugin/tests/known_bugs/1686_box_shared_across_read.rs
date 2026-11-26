// Known bug, from the closed 2.x fix PRs (cx/oracle3-1677-1680-1683-1686); kept as a RustViz 3 regression case.
// Issue(s): #1686 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let b = Box::new(String::new());
    let r2 = &*b;
    'outer: for i in 0..2 {
        for j in 0..2 {
            let r3 = &*b;
            if r2.is_empty() { break 'outer; }
        }
    }
    println!("{}", b);
    println!("{}", r2);
}
