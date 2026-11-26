// Known bug, from the closed 2.x fix PRs (fix/a-calls-1620-1645); kept as a RustViz 3 regression case.
// Issue(s): #1717 (see known_bugs/README.md for what main draws wrong).
// #1717 (known wrong, pinned in KNOWN_INVARIANT_FAILURES): a two-phase
// borrow whose call is written over several lines keeps main's order:
// push's reborrow is drawn on the `r.push(` line, before `r.len()` below
// it, though rustc runs the argument first. (Drawing it on the argument's
// line put the call's result on the wrong row, #1696's review.)
fn main() {
    let mut v = Vec::<usize>::new();
    let r = &mut v;
    r.push(
        r.len()
    );
    let n = v.remove(
        v.len() - 1
    );
    println!("{:?} {}", v, n);
}
