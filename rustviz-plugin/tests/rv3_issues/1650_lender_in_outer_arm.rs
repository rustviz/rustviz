fn run(c: bool, k: bool, s2: String) {
    if k {
        let v = vec![String::from("a")];
        if let Some(x) = v.first() {
            if c {
                drop(s2);
                let o = x.clone();
            }
        }
        v.len();
    }
}
