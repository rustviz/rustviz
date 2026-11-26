fn loop_in_arm_then_assign(c: bool, items: Vec<u8>) {
    let mut s;
    if c {
        for _ in items {
            s = String::new();
        }
    }
    s = String::from("last");
    println!("{}", s);
}
