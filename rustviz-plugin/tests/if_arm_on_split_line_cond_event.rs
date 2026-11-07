// #508, #509: the condition reads `s` on the `if` line as well as the arm
// moving it there, so the column has an event of its own on the split
// line. The condition runs before either arm: its dot is the split dot,
// half a row above the `if` row, and the `if` arm starts on that row with
// the move.
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    if s.len() > 1 { take(s); } else {
        println!("b");
    }
}
