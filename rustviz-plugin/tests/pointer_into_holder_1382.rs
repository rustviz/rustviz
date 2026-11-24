// #1382: a call that returns a reference into an element of a holder
// (`refs.last_mut().unwrap()` with `refs: Vec<&String>`) points into
// refs, as `&mut refs[0]` does; the `&String` stored there borrows a1,
// but el reaches a1 only through what it points to. el was drawn
// borrowing a1 too, mutably, and a1's column went `State::Invalid`.
fn f1() {
    let a1 = String::from("a1");
    let mut refs = vec![&a1];
    let el = refs.last_mut().unwrap();
    println!("{}", el);
    println!("{:?}", refs);
}

fn f2() {
    let a1 = String::from("a1");
    let refs = vec![&a1];
    let el = refs.last().unwrap();
    println!("{}", el);
    println!("{:?}", refs);
}

fn f3() {
    let a1 = String::from("a1");
    let mut refs = vec![&a1];
    let el = refs.get_mut(0).unwrap();
    println!("{}", el);
}

fn f4() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let el = refs.last_mut().unwrap();
    *el = &t1;
    println!("{:?}", refs);
}

fn f5() {
    let a1 = String::from("a1");
    let o = Some(&a1);
    let r = o.as_ref().unwrap();
    println!("{}", r);
}

// The reference copied out of the element borrows a1, not refs: refs
// can be changed while x is in use.
fn f6() {
    let a1 = String::from("a1");
    let a2 = String::from("a2");
    let mut refs = vec![&a1];
    let x = refs.last().copied().unwrap();
    refs.push(&a2);
    println!("{} {:?}", x, refs);
}

fn main() {
    f1();
    f2();
    f3();
    f4();
    f5();
    f6();
}
