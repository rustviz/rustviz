pub fn run(s: String, first: bool) {
    let tp = (String::from("ab"));
    if s.len() < 2 {
        let r = &s;
        drop(tp);
        if r.is_empty() {
            let mut r2 = r;
            if first {
                drop(r2);
            } else {
                println!("{r2} {s}");
            }
        } else {
        }
    }
}
