// Known bug, from the closed 2.x fix PRs (cx/hedge-f1); kept as a RustViz 3 regression case.
// Issue(s): #1681 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let mut v = vec![String::from("a")];
    {
        let o = v.last_mut();
        if let Some(e) = o {
            e.push('x');
        }
    }
    println!("{}", v.len());
    for e in v.iter_mut() {
        e.push('z');
    }
}
