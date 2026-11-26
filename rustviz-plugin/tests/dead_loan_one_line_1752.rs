fn run(mut s: String, t: String) {
    let mut r = &s;
    let n = r.len(); r = &t; s.push('x'); r = &s;
    println!("{}", r);
}
