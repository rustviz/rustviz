// #508: the `if` arm's first event (the move into `take`) is written on
// the `if` line. The arm's column starts on that row with the move, and
// the split dot sits half a row above, as for an `if let` binding (#382).
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    let c = true;
    if c { take(s); } else {
        println!("b");
        println!("{}", s);
    }
}
