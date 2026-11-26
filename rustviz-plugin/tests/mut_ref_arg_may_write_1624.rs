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
