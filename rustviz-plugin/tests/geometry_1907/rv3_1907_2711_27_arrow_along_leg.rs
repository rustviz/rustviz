struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn run(mut s: String, flag: bool) -> Option<usize> {
    let o = Some(String::from("x"));
    let s4 = o?;
    if s.len() > 1 {
        let h = holder(&s4);
    } else {
        let s6;
        if s.len() > 1 {
            s6 = s4;
        }
    }
    Some(0)
}
