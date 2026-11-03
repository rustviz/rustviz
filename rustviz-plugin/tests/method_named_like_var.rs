// #232: the other order. `len` is a variable first, then `s.len()`
// is called. The call used to resolve to the variable, so the i32
// `len` was shown being borrowed from s and copied into n.
fn main() {
    let len = 3;
    let s = String::from("hello");
    let n = s.len();
    println!("{} {} {}", len, s, n);
}
