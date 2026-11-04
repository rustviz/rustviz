// Passing an existing `&mut` to a closure call (#201): `ru` is
// reborrowed for the call, not captured by `g` (which captures
// nothing), and it happens once.

fn main() {
    let mut s = String::from("a");
    let ru = &mut s;
    let g = |q: &mut String| q.push_str("!");
    g(ru);
}
