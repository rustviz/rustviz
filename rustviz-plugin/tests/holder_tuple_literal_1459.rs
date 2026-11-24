// #1459: `let pair = Some((&a, &b));` was drawn borrowing nothing (a tuple
// literal inside `Some(..)` wasn't looked into), and the references a
// pattern copied out of pair then gave their borrow back "to pair". pair
// borrows a and b, as `Some(&a)` and `Some([&a, &b])` do; x is the `&a`
// stored in it and borrows a only (a tuple's elements keep their own
// lifetimes, unlike an array's), y borrows b.
fn let_else() {
    let a = String::from("a");
    let b = String::from("b");
    let pair = Some((&a, &b));
    let Some((x, y)) = pair else { panic!() };
    println!("{} {} {} {}", x, y, a, b);
}

// `if let` said "x's immutable borrow ends (what it borrows from isn't
// shown)".
fn if_let() {
    let c = String::from("c");
    let d = String::from("d");
    let cd = Some((&c, &d));
    if let Some((x1, y1)) = cd {
        println!("{} {}", x1, y1);
    }
    println!("{} {}", c, d);
}

// `Ok(..)`, given by an assignment.
fn ok_assigned() {
    let e = String::from("e");
    let g = String::from("g");
    let mut r: Result<(&String, &String), ()> = Err(());
    println!("{:?}", r);
    r = Ok((&e, &g));
    let Ok((x2, y2)) = r else { return };
    println!("{} {} {} {}", x2, y2, e, g);
}

// `&mut` elements: the holder is moved by the pattern, and gives h and k
// back on the `return` path.
fn mutable() {
    let mut h = String::from("h");
    let mut k = String::from("k");
    let hk = Some((&mut h, &mut k));
    let Some((x3, y3)) = hk else { return };
    x3.push('1');
    y3.push('2');
    println!("{} {}", h, k);
}

// A `vec!` of tuples of references.
fn in_vec() {
    let m = String::from("m");
    let n = String::from("n");
    let mn = vec![(&m, &n)];
    println!("{:?}", mn);
    println!("{} {}", m, n);
}

fn main() {
    let_else();
    if_let();
    ok_assigned();
    mutable();
    in_vec();
}
