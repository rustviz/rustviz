// `Box::new(String::from(..))` — issue #76 case 1.
// Should render `b` as a single owning column (like a `String` would),
// not as the `Box` ADT's recursive internals (`b.0.pointer.pointer`,
// `b.0._marker`, `b.1`, ...).

fn main() {
    let b = Box::new(String::from("hi"));
    println!("{}", b);
}
