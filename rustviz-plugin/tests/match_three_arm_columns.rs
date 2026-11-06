// #369: each `match` arm's column starts at its pattern's line and ends
// at its last line; the split and the join reach it with a diagonal.
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    let n = 3;
    match n {
        0 => {
            println!("zero");
            take(s);
        }
        1 => {
            println!("one");
        }
        _ => {
            println!("many");
            println!("{}", s);
        }
    }
}
