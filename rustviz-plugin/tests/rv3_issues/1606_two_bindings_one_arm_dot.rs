fn run(m: &mut String, v: Vec<String>) -> Option<usize> {
    let s = String::from("x");
    if let [x, rest @ ..] = v.as_slice() {
        let m2 = m;
        if x.is_empty() { return None; }
    }
    Some(0)
}
