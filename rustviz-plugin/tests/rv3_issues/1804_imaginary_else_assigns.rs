pub fn f(c: bool, t: (i32,)) {
    let mut s: String;
    if c {
        s = String::new();
        drop(s);
    }
    if let (_,) = t {
    } else {
        s = String::new();
    }
    let _k = 0;
}
