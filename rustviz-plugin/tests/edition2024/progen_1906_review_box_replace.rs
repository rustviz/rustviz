fn run() {
    let mut b = Box::new((String::from("first"), 7));
    let moved = b.0;
    assert_eq!(b.1, 7);
    b = Box::new((String::from("second"), 8));
    assert_eq!(b.1, 8);
    drop(moved);
}
