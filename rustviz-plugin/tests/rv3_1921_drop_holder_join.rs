struct H<'a>(&'a String);
impl Drop for H<'_> {
    fn drop(&mut self) {
        println!("{}", self.0);
    }
}

pub fn check(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let mut h = H(&b);
    let taken = c && {
        drop(h);
        true
    };
    println!("{taken}");
    h = H(&a);
    println!("{}", h.0);
}
