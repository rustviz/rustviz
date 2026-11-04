// A branch of type `!` never reaches the end of its conditional (#317).
// The `if bad` branch returns, so `t`'s column in it stops at the
// branch's end in a short bar instead of running on into the merge
// dot. (`t` is dropped at the `return`; that drop isn't drawn, #363.)
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
