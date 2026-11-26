fn run(v: Vec<String>, c: bool, e: bool, s2: String) {
    let other = v.first();
    if e {
        if let Some(x) = v.get(1) {
            if c {
                drop(s2);
                let o = x.clone();
            }
        }
    } else {
        v.len();
    }
    other.unwrap();
}
