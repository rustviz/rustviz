struct H<'a>(&'a String, &'a String);
impl Drop for H<'_> {
    fn drop(&mut self) {
        println!("{} {}", self.0, self.1);
    }
}

pub fn check(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let keep = &b;
    let mut h = H(&b, &b);
    h = if c {
        drop(h);
        H(&a, &a)
    } else {
        H(&a, &a)
    };
    println!("{} {} {keep}", h.0, h.1);
}

struct Single<'a>(&'a String);
impl Drop for Single<'_> {
    fn drop(&mut self) {
        println!("{}", self.0);
    }
}

pub fn independent_loan(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let keep = &b;
    let mut h = Single(&b);
    h = if c {
        drop(h);
        Single(&a)
    } else {
        Single(&a)
    };
    println!("{} {keep}", h.0);
}
