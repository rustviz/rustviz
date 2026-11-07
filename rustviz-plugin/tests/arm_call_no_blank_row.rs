// #419: a line with a call inside an `if` arm gets no blank row below it
// (it has none outside a conditional either).
fn id(s: String) -> String { s }

fn main() {
    let s = String::from("a");
    let c = true;
    if c {
        let n = id(s);
        println!("{}", n);
    } else {
        println!("c");
    }
}
