// #1371: a column mutably lent out across a conditional whose arms have
// events on it draws its legs dotted (#722). They start at a fork dot on
// the conditional's row and end at a join dot, as other columns' legs
// do; main forked and joined in mid-line, with "v is live in a
// conditional expression" on the arm's first dot and the join's text on
// the next dot after the conditional.
fn local_ref_if_let() {
    let mut v = vec![1, 2, 3];
    let r3 = &mut v;
    let o = r3.get_mut(0);
    if let Some(e) = o {
        *e += 1;
    }
    r3.push(4);
}
fn if_else_ends_loan(c: bool) {
    let mut s = String::from("hi");
    let r = &mut s;
    if c {
        r.push('x');
        println!("a");
    } else {
        println!("b");
    }
    println!("{}", s);
}
fn match_writes() {
    let mut v = vec![1, 2, 3];
    let r = &mut v;
    let o = r.get_mut(0);
    match o {
        Some(e) => *e += 1,
        None => {}
    }
    r.push(4);
}
fn main() {
    local_ref_if_let();
    if_else_ends_loan(true);
    match_writes();
}
