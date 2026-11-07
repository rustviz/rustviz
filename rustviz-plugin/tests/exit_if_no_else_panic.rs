// A panic in an `if` without `else` (#429): `v` is dropped as the
// panic unwinds, on that path only, after the message read it (#449).
// After the `if`, `v` is still there.

fn main() {
    let v = vec![1, 2, 3];
    if v.len() > 100 {
        panic!("too long: {}", v.len());
    }
    println!("{}", v.len());
}
