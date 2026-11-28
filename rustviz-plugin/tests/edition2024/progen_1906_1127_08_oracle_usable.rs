struct Pair {
    a: String,
    b: String,
}

fn take(_s: String) {}

fn id<T>(t: T) -> T {
    t
}

fn run(mut s: String, flag: bool) -> Option<usize> {
    let mut s3 = String::from("abc");
    let p = Pair { a: String::from("x"), b: s3.clone() };
    Pair { a: s3, b: s } = p;
    if s.len() > 1 {
        if flag { return None; }
        take(id(s));
    } else {
    }
    Some(0)
}
