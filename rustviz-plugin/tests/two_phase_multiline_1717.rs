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
