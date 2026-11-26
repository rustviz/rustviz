// Known bug, from the closed 2.x fix PRs (cx/oracle3-1677-1680-1683-1686); kept as a RustViz 3 regression case.
// Issue(s): #1686 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let b = Box::new(String::new());
    let x = b.as_str();
    match b.as_str() {
        "" => {}
        _ => {
            if x.len() > 1 {
                let r = &*b;
                println!("{}", b);
            } else if x.len() > 1 {
                println!("{}", b);
            }
        }
    }
}
