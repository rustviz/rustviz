fn run(r: &String, mut o: Option<String>) -> Option<usize> {
    let s2 = r.clone();
    let mut s3 = String::from("abc");
    let o2: Option<String> = None;
    let s4 = match o2 {
        Some(y) => y,
        None => String::new(),
    };
    if s2.len() > 1 {
    } else if r.is_empty() {
        let s6 = if s3.len() > 1 { format!("{}!", s4) } else { o.unwrap() };
        let m3 = &mut s3;
    }
    Some(0)
}
