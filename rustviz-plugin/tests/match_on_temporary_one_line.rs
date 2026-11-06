// A one-line `match` on a temporary (#459) panicked in the SVG
// generator (min > max). `c` is moved into the temporary `Some(c)` on
// the `match` line, before any arm; the `_` and `..` patterns bind
// nothing, so the String is dropped with the temporary.
fn main() {
    let c = String::from("c");
    match Some(c) { Some(_) => {}, None => {} }
    let d = String::from("d");
    match Some(d) { Some(..) => println!("some"), None => println!("none") }
}
