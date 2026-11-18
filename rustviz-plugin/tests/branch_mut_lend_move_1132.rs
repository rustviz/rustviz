// #1132: a value mutably lent in one branch and moved in another is
// maybe-moved after the join. The end of the borrow doesn't make it
// the owner again; only an assignment does.
fn lend_then_move(cond: bool) {
    let mut s = String::from("hello");
    let mut other = String::from("other");
    let mut r = &mut other;
    if cond {
        r = &mut s;
    } else {
        drop(s);
    }
    r.push_str("!");
}
fn in_match(n: i32) {
    let mut t = String::from("hello");
    let mut other = String::from("other");
    let mut r = &mut other;
    match n {
        0 => r = &mut t,
        1 => drop(t),
        _ => {}
    }
    r.push_str("!");
}
fn move_then_lend_reassigned(cond: bool) {
    let mut w = String::from("hello");
    let mut other = String::from("other");
    let mut r = &mut other;
    if cond {
        drop(w);
    } else {
        r = &mut w;
    }
    r.push_str("!");
    w = String::from("new");
    println!("{}", w);
}
fn main() {
    lend_then_move(true);
    in_match(0);
    move_then_lend_reassigned(false);
}
