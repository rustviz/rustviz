struct S<'a> {
    v: Vec<&'a i32>,
}
fn main() {
    let mut p = (1, 2);
    let mut s = S { v: Vec::new() };
    s.v.extend([&p.0]);
    println!("{}", s.v[0]);
}
