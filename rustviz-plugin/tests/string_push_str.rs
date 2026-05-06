// Calling a stdlib mutable-ref method (`push_str` is `&mut self`).
// Should produce a PassByMutableReference (rendered as
// "push_str reads from/writes to s").

fn main() {
    let mut s = String::from("hi");
    s.push_str("!");
    println!("{}", s);
}
