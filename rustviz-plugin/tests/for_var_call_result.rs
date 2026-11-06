// A `for` loop over a call result (#205). `c` is a fresh `char` each
// iteration: bound at the `for` line, out of scope at the body's
// closing brace. Before the fix its column had neither.

fn show(_ch: &char) {}

fn main() {
    let s = String::from("ab");
    for c in s.chars() {
        show(&c);
    }
}
