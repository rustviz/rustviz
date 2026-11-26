struct H<'a>(&'a String);
impl Drop for H<'_> { fn drop(&mut self) {} }
pub fn main() {
    let a = String::from("a");
    let mut b = String::from("b");
    let c = std::env::args().len() > 1;
    let mut h = H(&b);
    h = if c {
        drop(h);
        b.push('x');
        H(&a)
    } else {
        H(&a)
    };
    println!("{}", h.0);
}
