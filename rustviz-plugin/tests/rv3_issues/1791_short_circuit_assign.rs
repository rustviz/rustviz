fn f(c: bool, stop: bool) {
    let mut s = String::from("a");
    if c { drop(s); }
    let _ = stop && { s = String::new(); true };
    println!("end");
}
