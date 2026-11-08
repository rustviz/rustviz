// #760: a `move` closure returned from a function takes what it
// captures with it to the caller. The captured parameter (or local) is
// moved into the closure, not dropped when the function returns; the
// String is dropped when the caller drops the closure.

fn adder(s: String) -> impl Fn(usize) -> usize {
    move |k| s.len() + k
}

fn make(n: i32) -> impl Fn(i32) -> i32 {
    let label = String::from("label");
    move |k| n + k + label.len() as i32
}

fn main() {
    let t = String::from("t");
    let a = adder(t);
    println!("{}", a(1));
    let m = make(2);
    println!("{}", m(3));
}
