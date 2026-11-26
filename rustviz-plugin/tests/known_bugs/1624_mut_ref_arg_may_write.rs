// Known bug, from the closed 2.x fix PRs (fix/a-calls-1620-1645); kept as a RustViz 3 regression case.
// Issue(s): #1624 (see known_bugs/README.md for what main draws wrong).
// #1624: passing a `&mut` reference reborrows what it points to, which
// lets the callee read and write it; RustViz doesn't look at the callee's
// body (`show` only reads).
fn show(v: &mut Vec<i32>) {
    println!("{:?}", v);
}

fn main() {
    let mut x = vec![1];
    let r = &mut x;
    show(r);
    r.push(2);
}
