// A `fn` pointer in a local (#254). Calling `f(s)` moves `s` into the
// call, and `t` gets the call's result; nothing is copied or moved
// out of `f` itself.

fn shout(s: String) -> String { s.to_uppercase() }

fn main() {
    let s = String::from("hi");
    let f: fn(String) -> String = shout;
    let t = f(s);
    println!("{}", t);
}
