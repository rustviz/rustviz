fn run(mut s: String) {
    let mut r = &s;
    if r.is_empty() {
        drop(s);
        s = String::new();
    }
    r = &s;
}
