// #579: a move and an assignment inside an `if` within a chain part
// last on the path into the else branch too: there `s` may have been
// moved and `z` may have been assigned.

fn take(s: String) -> bool { s.len() > 0 }

fn main() {
    let s = String::from("a");
    let y = Some(5);
    let mut z = 0;
    if let Some(a) = y && { if a > 2 { z = a; take(s); } z > 3 } {
        println!("big");
    } else {
        println!("{}", z);
    }
}
