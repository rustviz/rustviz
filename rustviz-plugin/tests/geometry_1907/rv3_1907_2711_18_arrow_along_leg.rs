struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn take(_s: String) {}

fn run(mut s: String, mut v: Vec<String>) -> Option<usize> {
    let s2 = std::mem::take(&mut s);
    let h = Holder { r: &s2, n: 1 };
    if s2.len() > 1 {
        match v.as_slice() {
            [] => {
                let s4 = String::from("abc");
                if s4.len() > 1 {
                    return None;
                }
            }
            [x2] => {}
            [x21, rest @ ..] => {}
        }
    } else {
        let m = &mut s;
    }
    Some(0)
}
