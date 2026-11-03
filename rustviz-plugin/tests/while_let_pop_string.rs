// The playground's "while let popping a stack" example (#192). `s` is a
// `String` bound by a pattern; it used to be registered as a struct with
// no field columns, and the timeline drew an empty struct box with
// width="-80".
fn consume(_s: String) {}

fn main() {
    let mut stack = vec![String::from("a"), String::from("b")];
    while let Some(s) = stack.pop() {
        consume(s);
    }
}
