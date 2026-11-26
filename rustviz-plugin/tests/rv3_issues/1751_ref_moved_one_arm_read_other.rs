fn repointed_then_moved(c: bool) {
    let mut a20 = String::from("a");
    let mut b20 = String::from("b");
    let mut r20 = &mut a20;
    if c {
        r20 = &mut b20;
        let next20 = r20;
        drop(a20);
        next20.push('x');
    } else {
        r20.len();
    }
}
