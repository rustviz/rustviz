// #737: `[a, b].len()` gives len the value `len()` returns (not a copy
// of a variable named len); `(u, 1).1` is a copy of the tuple's i32.
// #758: `Some(5)` in a pattern is a constructor, not a call that returns.

fn make() -> String { String::from("m") }

fn main() {
    let u = make();
    let k = (u, 1).1;
    let a = make();
    let b = make();
    let len = [a, b].len();
    let m = make().len();
    println!("{} {} {}", k, len, m);
    if let Some(x) = Some(5) {
        println!("{}", x);
    }
}
