struct H<'a>(&'a String);
impl Drop for H<'_> { fn drop(&mut self) {} }

fn check(c: bool) {
    let a = String::from("a");
    let mut b = String::from("b");
    let mut h = H(&b);
    if c { let _ = b.len(); } else { drop(h); b.clear(); h = H(&a); }
    h = H(&a);
    println!("{}", h.0);
}
