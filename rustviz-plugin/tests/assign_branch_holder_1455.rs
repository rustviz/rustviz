// #1455: a holder assigned an `if` whose branch builds its value from a
// reference the branch declares (`let r2 = &a; Some(r2)`). q may hold
// r2's reference (into a) or one into b, so both stay borrowed until
// q's last use. The copy of r2 into `Some` is the holder's borrow, not
// a copy into the constructor. Each branch's value goes to q in that
// branch, so the then-branch's borrow of a is drawn on a's leg there
// (#1463).
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut q = None;
    println!("{:?}", q);
    q = if c {
        let r2 = &a;
        Some(r2)
    } else {
        Some(&b)
    };
    println!("{:?}", q);

    let mut v: Vec<&String> = Vec::new();
    println!("{:?}", v);
    v = if c {
        let r = &a;
        vec![r]
    } else {
        vec![&b]
    };
    println!("{:?}", v);

    let mut n = None;
    println!("{:?}", n);
    n = if c {
        let k = 5;
        Some(k)
    } else {
        None
    };
    println!("{:?}", n);
}
