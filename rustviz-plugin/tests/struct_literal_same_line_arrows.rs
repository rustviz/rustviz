// A call result and a variable moved into fields on the same line:
// both arrows must render (the `String::from` one used to be dropped
// whenever the line also had a variable-to-variable arrow).
struct S { b: String, c: String }
fn main() {
    let t = String::from("t");
    let s = S { b: String::from("x"), c: t };
    println!("{}", s.b);
}
