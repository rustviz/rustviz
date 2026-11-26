fn f(c: bool) {
    let (mut x, mut y) = (0, 1);
    let mut r = &mut x;
    if c { r = &mut y; }
    println!("{}", *r);
}
