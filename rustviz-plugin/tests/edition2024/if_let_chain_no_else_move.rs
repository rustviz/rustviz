// #281 without an `else`: when `a.len() > 0` is false the body is
// skipped, but the String was already moved out of `x` into `a`, and
// is dropped with `a` as the condition fails. The path that skips the
// body starts with `x` maybe-moved.

fn consume(_o: String) {}

fn main() {
    let x = Some(String::from("a"));
    if let Some(a) = x && a.len() > 0 {
        consume(a);
    }
}
