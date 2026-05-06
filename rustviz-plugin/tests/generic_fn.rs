// A generic identity function. The String passed in moves to the
// parameter, then moves back to the caller as the return value.

fn id<T>(x: T) -> T {
    x
}

fn main() {
    let s = String::from("hi");
    let t = id(s);
    println!("{}", t);
}
