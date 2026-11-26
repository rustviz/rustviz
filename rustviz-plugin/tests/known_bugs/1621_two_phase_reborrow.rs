// Known bug, from the closed 2.x fix PRs (fix/a-calls-1620-1645); kept as a RustViz 3 regression case.
// Issue(s): #1621 (see known_bugs/README.md for what main draws wrong).
// #1621: a two-phase borrow through a `&mut` reference: push's reborrow
// of `*r` starts after its argument `r.len()` has run. The same for a
// `&mut` argument that a later argument reads (`add_len(r, r.len())`).
// An argument that re-points the receiver (`q.push({ q = &mut b; .. })`)
// keeps main's order: the call reserved the old referent before it ran
// (#1717).
fn add_len(r: &mut Vec<usize>, n: usize) {
    r.push(n);
}

fn main() {
    let mut v = Vec::<usize>::new();
    let r = &mut v;
    r.push(r.len());
    add_len(r, r.len());
    println!("{:?}", v);
    let mut a = vec![1];
    let mut b = vec![2];
    let mut q = &mut a;
    q.push({ q = &mut b; q.len() });
    println!("{:?} {:?}", a, b);
}
