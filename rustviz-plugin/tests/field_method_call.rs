// Regression for #71: chaining a method call onto a struct field.
// Should emit `push_str reads from/writes to r.s` (the
// PassByMutableReference path).

struct R { s: String }

fn main() {
    let mut r = R { s: String::from("hi") };
    r.s.push_str("!");
    println!("{}", r.s);
}
