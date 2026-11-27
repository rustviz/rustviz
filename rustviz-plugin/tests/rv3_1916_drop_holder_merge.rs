struct H<'a>(&'a String);
impl Drop for H<'_> { fn drop(&mut self) {} }

fn check(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let mut h = H(&b);
    if c {
        drop(h);
        h = H(&a);
    }
    h = H(&a);
    println!("{}", h.0);
}
