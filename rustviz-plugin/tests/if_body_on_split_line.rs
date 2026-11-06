// #369: the `if` arm is written on the `if` line itself, the `else`
// arm below it. The `if` arm's move is drawn on that line, and the
// `else` column starts below it.
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    let c = true;
    if c { take(s); } else {
        println!("b");
        println!("{}", s);
    }
}
