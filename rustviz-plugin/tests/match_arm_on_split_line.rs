// #508: a `match` whose first arm is written on the `match` line, with
// its move into `take` as that arm's first event. The arm's column starts
// on the `match` row with the move, not a row below it.
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    let n = 1;
    match n { 0 => take(s),
        _ => {
            println!("other");
            println!("{}", s);
        }
    }
}
