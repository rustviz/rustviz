// A closure made on a line with other stacked arrows (a one-line `if
// let` arm: the pattern's borrow, the closure's capture, both borrows'
// ends): c's binding was put on the row below the stack, with its scope
// end, so the capture's arrow ended at no dot of c and c's column
// started in mid-air (#1420).
fn if_let_arm() {
    let v = vec![String::from("a")];
    if let Some(_x) = v.first() { let _c = || v.len(); }
}

fn match_arm() {
    let v = vec![String::from("a")];
    match v.first() { Some(_x) => { let _c = || v.len(); } None => {} }
}

fn mutable_capture() {
    let mut v = vec![String::from("a")];
    let o = Some(1);
    if let Some(_x) = o.as_ref() { let mut c = || v.push(String::new()); c(); }
}

fn two_captures() {
    let v = vec![String::from("a")];
    let s = String::from("b");
    if let Some(_x) = v.first() { let _c = || v.len() + s.len(); }
}

fn no_conditional() {
    let v = vec![String::from("a")];
    let _x = v.first(); let _c = || v.len();
}

fn main() {
    if_let_arm();
    match_arm();
    mutable_capture();
    two_captures();
    no_conditional();
}
