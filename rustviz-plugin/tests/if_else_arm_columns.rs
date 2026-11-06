// #369: each arm's column runs on its own lines only. The `else`
// column used to run alongside the `if` arm from the split down, so
// hovering it underlined `println!("a")` and `take(s)`. Now the
// split's diagonal goes straight to `} else {`.
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    let c = true;
    if c {
        println!("a");
        take(s);
    } else {
        println!("b");
        println!("{}", s);
    }
}
