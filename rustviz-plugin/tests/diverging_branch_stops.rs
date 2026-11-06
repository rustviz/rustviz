// A branch of type `!` never reaches the end of its conditional (#317).
// The `if bad` branch returns, so `t`'s column in it doesn't run on
// into the merge dot. It ends at the `return`, where `t` and the
// parameter `s` are dropped (#363).
fn check(s: String, bad: bool) -> usize {
    let t = String::from("x");
    if bad {
        println!("{}", t);
        return 0;
    } else {
        drop(t);
    }
    s.len()
}

fn main() {
    println!("{}", check(String::from("a"), false));
}
