struct P { a: String, b: String }
fn main() {
    let mut p = P { a: String::new(), b: String::new() };
    let g = || { let r = &p.a; let _ = p.b.len(); r };
    let r = g();
    p.b.clear();
    println!("{r}");
}
