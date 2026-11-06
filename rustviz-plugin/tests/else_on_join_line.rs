// #369: the `else` arm is written on the line of the closing brace.
// Its move is drawn on that line, not beside the `if` arm's lines.
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    let c = true;
    if c {
        println!("a");
        println!("{}", s);
    } else { take(s); }
}
