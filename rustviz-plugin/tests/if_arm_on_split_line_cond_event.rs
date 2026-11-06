// #508: the condition reads `s` on the `if` line as well as the arm
// moving it there, so the column has an event of its own on the split
// line. Drawn as before this fix (the column's dot would otherwise be
// left between the arms); only an arm whose first event is the sole
// event on the split line is drawn from a raised split.
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    if s.len() > 1 { take(s); } else {
        println!("b");
    }
}
