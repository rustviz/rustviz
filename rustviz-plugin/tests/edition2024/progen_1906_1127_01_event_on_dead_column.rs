fn run(mut o: Option<String>, v: Vec<String>) -> Option<usize> {
    let o2: Option<String> = None;
    if o.is_some() {
        let c = move || v.len();
        let Some(x) = &o2 else {
            return None;
        };
    }
    Some(0)
}
