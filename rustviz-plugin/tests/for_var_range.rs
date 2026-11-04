// A `for` loop over a range (#205): `i` is bound at the `for` line and
// goes out of scope at the body's closing brace.

fn take(_k: i32) {}

fn main() {
    let n = 3;
    for i in 0..n {
        take(i);
    }
    take(n);
}
