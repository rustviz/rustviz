fn run(r: &String, o: Option<String>, flag: bool) -> Option<usize> {
    let s = format!("{}!", r);
    let vr = vec![s.as_str(), "abc"];
    if s.len() > 1 {
    } else if flag {
        let Some(x) = o else {
            return None;
        };
    } else {
        let x2 = &s[..];
    }
    Some(0)
}
