// pair is borrowed twice through refs: an immutable borrow of one field and
// a mutable borrow of the other. Each exit leg and the join give pair back
// the mutable borrow first, then the shared one.
fn panic_leg() {
    let mut pair = ([(1, 2)], 3);
    let mut refs = Vec::new();
    refs.push((&pair.0[0].0, &mut pair.0[0].1));
    *refs[0].1 = 8;
    assert_eq!(*refs[0].0, 1);
}

fn return_leg() {
    let mut pair = ([(1, 2)], 3);
    let mut refs = Vec::new();
    refs.push((&pair.0[0].0, &mut pair.0[0].1));
    if refs.len() > 3 {
        return;
    }
    println!("{}", refs[0].0);
}

fn main() {
    panic_leg();
    return_leg();
}
