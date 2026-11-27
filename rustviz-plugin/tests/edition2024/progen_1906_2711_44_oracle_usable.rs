struct Pair {
    a: String,
    b: String,
}

fn take(_s: String) {}

fn len_opt(s: &String) -> Option<usize> {
    s.chars().next().map(|_| s.len())
}

fn run(mut s: String, mut o: Option<String>, mut v: Vec<String>) -> Option<usize> {
    let s2 = std::mem::take(&mut s);
    let p = Pair { a: o.unwrap(), b: String::from("ab") };
    let o2 = Some(p.a);
    let s4 = s2;
    if let Some(x) = o2.as_ref() {
        let s5 = s4;
        while let Some(x3) = v.pop() {
            let n2 = len_opt(&x3)?;
        }
    }
    Some(0)
}
