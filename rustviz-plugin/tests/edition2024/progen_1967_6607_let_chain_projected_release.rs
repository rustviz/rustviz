struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn run(m: &mut String, mut o: Option<String>) -> Option<usize> {
    let s = String::from("ab");
    let s6 = s;
    let mut r3 = &*m;
    if s6.len() > 1 && let Some(x2) = o && x2.len() > 1 {
        let c2 = || r3.len();
    } else {
        let h = Holder { r: r3, n: 1 };
    }
    Some(0)
}
