fn f(c: bool) {
    let mut either = String::from("a");
    let both_old = either;
    'block: {
        if c {
            either = String::from("b");
            break 'block;
        } else {
            break 'block;
        }
    }
    drop(both_old);
}
