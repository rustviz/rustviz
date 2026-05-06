// Regression for #72: moving a nested struct field by value.
// Should emit `Move from r.a.b to x`.

struct A { b: String }
struct R { a: A }

fn main() {
    let r = R { a: A { b: String::from("hi") } };
    let x = r.a.b;
    println!("{}", x);
}
