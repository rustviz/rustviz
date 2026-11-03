// Deferred initialization of an owned value (#234). `s = ...` moves a
// fresh String into s. s held nothing before, so nothing is dropped
// at the assignment.

fn main() {
    let s;
    s = String::from("hello");
    println!("{}", s);
}
