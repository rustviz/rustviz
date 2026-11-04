// A closure that mutates what it captured (#133): each call runs
// the body, which can modify `s` through the closure's mutable
// borrow. `s` still owns its String afterwards and drops it.

fn main() {
    let mut s = String::from("a");
    let mut push = || s.push_str("b");
    push();
    push();
    println!("{}", s);
}
