fn run(mut o: Option<String>, flag: bool) -> Option<usize> {
    let s = String::from("abc");
    let x = s.as_str();
    if let Some(x4) = &o && x4.len() > 1 {
    } else {
        if flag { let n = x.len(); } else { println!("{:?}", o); }
    }
    Some(0)
}
