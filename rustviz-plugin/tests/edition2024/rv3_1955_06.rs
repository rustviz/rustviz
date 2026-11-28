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

fn run() -> Option<usize> {
    let s2 = String::from("rust");
    let mut o = Some(id(s2));
    if let Some(x) = o.as_mut() {
    } else {
        if let Some(x2) = &mut o {
        } else {
            let h = holder(o.as_ref().unwrap());
        }
    }
    Some(0)
}
