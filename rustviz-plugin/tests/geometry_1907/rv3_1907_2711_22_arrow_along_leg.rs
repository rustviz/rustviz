fn run(s: String, o: Option<String>, flag: bool) -> Option<usize> {
    let s2 = String::from("hi");
    if let Some(x) = o.as_ref() {
    } else {
        if flag { return Some(s.len()); }
    }
    Some(0)
}
