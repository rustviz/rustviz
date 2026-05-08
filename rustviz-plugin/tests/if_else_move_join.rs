// Conditional join state — issue #108. `s` is moved on one branch
// and only borrowed on the other; at the merge `s` is "may have
// been moved" (Rust treats it as moved regardless of branch).

fn consume(_s: String) {}

fn main() {
    let s = String::from("hi");
    let cond = true;
    if cond {
        consume(s);
    } else {
        println!("kept: {}", s);
    }
}
