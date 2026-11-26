fn run(c: bool, d: bool) {
    let s = String::from("a");
    let q = &s;
    let r = &s;
    for _i in 0..4 {
        if c { } else if d { } else { let k = r.len(); }
    }
    println!("{}", q);
}
