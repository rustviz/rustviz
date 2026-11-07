// #603: `v[0] = a` lasts on the path that skips the body: after the
// `if`, `v[0]` is 5 when `a > 7` failed, and still 0 when `y` was None.

fn main() {
    let y = Some(5);
    let mut v = vec![0, 1];
    if let Some(a) = y && { v[0] = a; a > 7 } {
        println!("big");
    }
    println!("{:?}", v);
}
