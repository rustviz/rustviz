// Reassigning a binding that already owns a heap resource.
// The previously-held resource is dropped before the new one is bound.

fn main() {
    let x = String::from("hello");
    let mut y = String::from("test");
    y = x;
    println!("{}", y);
}
