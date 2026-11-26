fn run(c: bool) {
    let mut s = String::from("a");
    let r = &mut s;
    r.len(); if c { s.push('a'); } else { r.len(); }
}
