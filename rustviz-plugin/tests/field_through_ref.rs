// Regression for #73: borrowing a field through a reference.
// `&(&r).s` should produce the same arrows as `&r.s` —
// `Immutable borrow from r.s to p`.

struct R { s: String }

fn main() {
    let r = R { s: String::from("hi") };
    let p = &(&r).s;
    println!("{}", p);
}
