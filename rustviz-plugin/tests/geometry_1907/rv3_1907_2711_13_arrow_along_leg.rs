struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let s = String::from("abc");
    let s2 = 'blk: {
        s
    };
    let b = Box::new(s2);
    if b.len() > 2 {
        let h = holder(&b);
    } else if b.len() > 2 {
        let mut r3 = &*b;
    } else {
        let s4;
        s4 = String::from("hi");
    }
}
