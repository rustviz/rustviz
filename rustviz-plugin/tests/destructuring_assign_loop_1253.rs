// #1253: a destructuring assignment of a `loop`'s value takes apart the
// value of the `break` that ended the loop, as `let (x, n) = loop { … }`
// does (#452): s moves into x only if the loop ended with `break (s, 1)`.
fn assign_loop(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let mut x = String::new();
    let mut n = 0;
    (x, n) = loop {
        if c {
            break (s, 1);
        }
        break (t, 2);
    };
    println!("{} {}", x, n);
}
fn assign_loop_one() {
    let u = String::from("u");
    let mut y = String::new();
    let mut m = 0;
    (y, m) = loop {
        break (u, 1);
    };
    println!("{} {}", y, m);
}
fn main() {}
