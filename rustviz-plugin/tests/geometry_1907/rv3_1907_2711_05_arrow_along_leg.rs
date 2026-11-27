fn take(_s: String) {}

fn run(mut o: Option<String>) -> Option<usize> {
    let s = String::from("x");
    let mut s2 = s;
    let s3 = if s2.len() > 1 { String::from("x") } else { std::mem::take(&mut s2) };
    if let Some(x) = &o && x.len() > 1 {
        let r2 = &s2;
    } else {
        if o.is_some() { return Some(s3.len()); }
    }
    let s4;
    s4 = String::from("hi");
    Some(0)
}
