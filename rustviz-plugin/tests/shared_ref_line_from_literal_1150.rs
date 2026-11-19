// A shared reference bound to a value that borrows no variable here (a
// string literal, a `&'static str` call result, a const) can only read,
// like one made by `&a`: its line is hollow throughout. With `let mut` it
// was drawn solid until its first read, hollow after it, and solid again
// after each reassignment (#1150).
const GREETING: &str = "hi";
struct H<'a> { r: &'a str }
fn name() -> &'static str { "n" }
fn main() {
    let mut r: &str = "hello";
    println!("{}", r);
    r = "world";
    println!("{}", r);
    let mut q = "lit";
    println!("{}", q);
    q = name();
    println!("{}", q);
    let c = true;
    let mut w = if c { "a" } else { "b" };
    println!("{}", w);
    w = GREETING;
    println!("{}", w);
    let mut h = H { r: "x" };
    println!("{}", h.r);
    h.r = "y";
    println!("{}", h.r);
    let mut t = ("p", 1);
    println!("{}", t.0);
    t.0 = "q";
    println!("{}", t.0);
    let k: &str = "k";
    println!("{}", k);
}
