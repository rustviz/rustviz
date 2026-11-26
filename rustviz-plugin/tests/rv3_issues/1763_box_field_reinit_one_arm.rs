struct P { a: String, b: String }
fn reinit(mut b: Box<P>, flag: bool) {
    let x = b.a;
    if flag {
        b.a = String::new();
    }
    println!("{} {}", x, b.b);   // drop(b) here: E0382 (b.a may be uninitialized)
}
