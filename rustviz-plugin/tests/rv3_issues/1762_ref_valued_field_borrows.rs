struct H<'a> { s: &'a i32, t: String }
fn refpat(h: H<'_>) {
    let H { ref s, .. } = h;
    let n = 1;
    println!("{} {}", s, n);
}
fn closure(h: H<'_>) {
    let c = || h.s;
    let n = 1;
    println!("{} {}", c(), n);
}
fn boxed(b: Box<H<'_>>) {
    let x = &b.s;
    let n = 1;
    println!("{} {}", x, n);
}
