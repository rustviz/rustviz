// A String passed by value to a closure call is moved into the call
// (into the parameter `x`), not captured by `consume`; `consume`
// owns nothing and drops nothing.

fn main() {
    let s = String::from("a");
    let consume = |x: String| println!("{}", x);
    consume(s);
}
