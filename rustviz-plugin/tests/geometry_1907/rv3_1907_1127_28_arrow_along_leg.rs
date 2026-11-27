fn run(o: Option<String>) -> Option<usize> {
    let mut s2 = String::from("abc");
    if let Some(x) = &o && x.len() > 1 {
        if s2.len() > 1 { let m2 = &mut s2; } else { }
    } else {
    }
    Some(0)
}
