struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn id<T>(t: T) -> T {
    t
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let s = String::from("hey");
    let s2 = id(s);
    let h = holder(&s2);
    if s2.len() > 1 {
        let x = &s2[..];
        if s2.len() > 1 { return; }
    } else {
        println!("{}", h.n);
    }
    let s4;
    s4 = String::from("rust");
}
