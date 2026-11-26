// Known bug, from the closed 2.x fix PRs (cx/hedge-t1); kept as a RustViz 3 regression case.
// Issue(s): #1642 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let mut chain = Box::new(0);
    if let Some(value) = std::hint::black_box(Some(1)) && value > 0 {
        *chain = value;
    } else {
        *chain = 2;
    }
    assert_eq!(*chain, 1);
}
