fn run(t: String) {
    let mut s = String::from("rust");
    let mut r2 = &t;
    if s.len() > 1 { } else { r2 = &mut s; }
}
