fn run(mut o: Option<String>, flag: bool) -> Option<usize> {
    let s = String::from("abc");
    let v: Vec<String> = Vec::new();
    if flag && let Some(x) = &o && x.len() > 1 {
        let Some(x2) = v.last() else { return None; };
        let c = move || s;
    }
    Some(0)
}
