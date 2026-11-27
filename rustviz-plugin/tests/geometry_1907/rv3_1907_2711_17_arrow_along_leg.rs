fn run(mut o: Option<String>, flag: bool) -> Option<usize> {
    let m = o.as_mut().unwrap();
    if flag {
        let s2 = match o {
            Some(y) if y.len() > 1 => y,
            Some(_) => String::new(),
            None => String::new(),
        };
    } else {
        m.push_str("ab");
    }
    Some(0)
}
