// #1262: a let-chain part that moves a reference on and lends it
// (`&& { let wr = r; take(wr); true } &&`) runs on the way into the `if`
// arm and is drawn there. wr's borrow ends there, after `take` borrowed
// through it, and v gets its value back then; on the other path r's own
// borrow ends in the other arm. Main drew both ends on the `if` line's
// own row, above the arm: wr's borrow ending before wr was bound, and r's
// before r was moved into wr.
fn take(_s: &mut String) {}
fn no_else(o: Option<i32>) {
    let mut v = String::from("v");
    let r = &mut v;
    if let Some(x) = o && { let wr = r; take(wr); true } && x > 5 {
        println!("y");
    }
}
fn with_else(o: Option<i32>) {
    let mut v = String::from("v");
    let r = &mut v;
    if let Some(x) = o && { let wr = r; take(wr); true } && x > 5 {
        println!("y");
    } else {
        println!("n");
    }
    println!("{}", v);
}
fn main() {
    no_else(Some(6));
    with_else(None);
}
