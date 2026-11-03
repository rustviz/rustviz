// #231: same cause as struct_after_fn_hashes.rs. Here one of main's
// variables got the hash of helper's `String::from` (a Function RAP,
// which has no column), and the renderer panicked looking it up.
fn helper() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() + b.len();
    println!("{}", c);
    let d = &a;
    let e = &b;
    println!("{} {}", d, e);
}

struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let p = Point { x: 1, y: 2 };
    let s = String::from("hi");
    let t = s;
    let u = &t;
    println!("{} {} {}", p.x, t, u);
}
