// #1333: a `&mut` declared in an arm (its pattern binding, or a `let`
// in it) and moved into another reference there hands its borrow on
// for good: the new reference gives the lender back, and the moved
// one draws no borrow end of its own after the move.
fn block_arm() {
    let mut s = String::from("hi");
    let o: Option<&mut String> = Some(&mut s);
    match o { Some(x) => { let r = x; r.push('!'); } None => {} }
    println!("{}", s);
}

fn if_let_arm() {
    let mut s2 = String::from("hi");
    let o2: Option<&mut String> = Some(&mut s2);
    if let Some(x2) = o2 {
        let r2 = x2;
        r2.push('?');
    }
    println!("{}", s2);
}

fn let_in_arm(c: bool) {
    let mut s3 = String::from("a");
    let mut t3 = String::from("b");
    let r3 = if c { let y3 = &mut s3; y3 } else { &mut t3 };
    r3.push('!');
    println!("{} {}", s3, t3);
}

fn main() {
    block_arm();
    if_let_arm();
    let_in_arm(true);
}
