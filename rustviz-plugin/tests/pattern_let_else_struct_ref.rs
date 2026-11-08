struct Part<'a> {
    part: &'a str,
}
fn main() {
    let mut s = String::from("hello");
    let o = Some(Part { part: &s });
    let Some(y) = o else { return };
    println!("{}", y.part);
    // y.part's last use was above, so s is free to change again.
    s.push('!');
    let t = String::from("world");
    let p = Some(&t);
    let Some(r) = p else { return };
    println!("{}", r);
}
