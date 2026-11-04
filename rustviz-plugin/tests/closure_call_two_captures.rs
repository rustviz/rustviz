// One call, two captured variables, one mutated and one read (#133).
// Both capture borrows end at the call, which stacks two return
// arrows on that line; the call's dots stay on the call's line.

fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut f = || {
        a.push_str(&b);
    };
    f();
    println!("{} {}", a, b);
}
