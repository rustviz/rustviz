fn else_if_bool(a: bool, b: bool) {
    let mut s = String::from("x");
    let m = &mut s;
    if a {
    } else if b {
        let n = m.len();
    }
    println!("x");
}
