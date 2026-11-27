struct H<'a>(&'a String);
impl Drop for H<'_> { fn drop(&mut self) {} }

fn check(c: bool, d: bool) {
    let a = String::from("a");
    let mut b = String::from("b");
    let mut h = H(&b);
    h = if c && d {
        drop(h);
        b.push('!');
        H(&a)
    } else {
        H(&a)
    };
    println!("{}", h.0);
}
