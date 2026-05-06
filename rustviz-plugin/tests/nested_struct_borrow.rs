// Regression for #72: borrowing a nested struct field.
// Should emit `Immutable borrow from r.a.b to p`.

struct A { b: String }
struct R { a: A }

fn main() {
    let r = R { a: A { b: String::from("hi") } };
    let p = &r.a.b;
    println!("{}", p);
}
