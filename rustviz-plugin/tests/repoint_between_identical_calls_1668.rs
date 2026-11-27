fn before_and_after() {
    let a = String::from("a");
    let b = String::from("b");
    let mut r = &a;
    let first = r.len(); r = &b; let second = r.len();
}
