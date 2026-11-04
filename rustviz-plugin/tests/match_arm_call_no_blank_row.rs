// #419: the same in a `match` arm, with a call that takes and returns.
fn id(s: String) -> String { s }

fn main() {
    let s = String::from("a");
    let n = 1;
    match n {
        0 => {
            let t = id(s);
            println!("{}", t);
        }
        _ => {
            println!("other");
        }
    }
}
