struct H<'a>(&'a String);
impl Drop for H<'_> { fn drop(&mut self) { println!("{}", self.0); } }
struct Wrap<'a>(H<'a>);

fn check(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let mut h = Wrap(H(&b));
    h = if c {
        Wrap(H(&a))
    } else {
        Wrap(H(&a))
    };
    println!("{}", h.0.0);
}
