struct H<'a>(&'a String);
impl Drop for H<'_> { fn drop(&mut self) {} }
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut h = H(&b);
    h = if c {
        let r = &a;
        H(r)
    } else {
        H(&a)
    };
    println!("{}", h.0);
}
