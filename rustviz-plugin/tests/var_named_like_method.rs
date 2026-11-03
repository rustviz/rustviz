// #232: `let len = s.len();`. The variable `len` used to be merged
// into the RAP of the `len` method, so it got no column.
fn main() {
    let s = String::from("hello");
    let len = s.len();
    println!("{} {}", s, len);
}
