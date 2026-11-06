// A `for` loop over a `Vec<String>` by value (#205). `s` is a fresh
// String each iteration and, owning it, is dropped at the body's closing
// brace every iteration.

fn show(_s: &String) {}

fn main() {
    let v = vec![String::from("a")];
    for s in v {
        show(&s);
    }
}
