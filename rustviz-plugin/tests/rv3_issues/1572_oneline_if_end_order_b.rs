fn run(c: bool) {
    let mut s2 = String::from("rust");
    let m2 = &mut s2;
    let n = if c { s2.len() } else { m2.len() };
}
