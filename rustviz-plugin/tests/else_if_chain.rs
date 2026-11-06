// An `else if` chain (#355). The inner `if` belongs to the outer
// `else` branch; picking events by source line put it in the outer
// `if` branch, because `} else if n > 1 {` is that branch's last line.
fn consume(_s: String) {}

fn main() {
    let n = 3;
    let s = String::from("hi");
    if n > 5 {
        consume(s);
    } else if n > 1 {
        println!("{}", s);
    } else {
        consume(s);
    }
}
