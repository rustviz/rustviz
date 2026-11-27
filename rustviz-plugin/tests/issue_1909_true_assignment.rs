fn run(move_first: bool) {
    let mut s = String::from("hi");
    if move_first {
        drop(s);
    }
    if true {
        s = String::from("rust");
    } else {
        println!("skipped");
    }
}
