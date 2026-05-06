// Regression for #72: passing the inner struct of a nested type
// to a free function by reference. Should emit
// `read_a reads from r.a` (the PassByStaticReference path).

struct A { b: String }
struct R { a: A }

fn read_a(_x: &A) {}

fn main() {
    let r = R { a: A { b: String::from("hi") } };
    read_a(&r.a);
}
