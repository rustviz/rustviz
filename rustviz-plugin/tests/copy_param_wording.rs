// #505: a Copy parameter gets a copy of the caller's value; nothing is
// owned. A String parameter does acquire ownership.
fn pick(c: bool, a: Option<i32>) -> i32 {
    if c { a.unwrap_or(0) } else { 0 }
}
fn take(s: String) -> usize {
    s.len()
}
fn main() {
    println!("{}", pick(true, Some(2)));
    println!("{}", take(String::from("hi")));
}
