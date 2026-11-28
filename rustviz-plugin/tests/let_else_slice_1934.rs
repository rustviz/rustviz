fn run() -> Option<usize> {
    let v: Vec<String> = Vec::new();
    let [x, ..] = v.as_slice() else { return None; };
    Some(0)
}
