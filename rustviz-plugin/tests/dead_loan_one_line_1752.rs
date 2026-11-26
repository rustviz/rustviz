// Straight-line code: no path skips line 3.
fn f(mut s: String, t: String) {
    let mut r = &s;
    let n = r.len(); r = &t; s.push('x'); r = &s;
    println!("{} {}", r, n);
}
