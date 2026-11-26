fn no_move(c: bool, d: bool, stop: bool) {
    let mut z;
    if c {
        z = String::new();
    }
    if d {
        z = String::new();
    }
    if stop { return; }
    z = String::from("last");
    println!("{}", z);
}
