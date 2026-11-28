struct Pair {
    a: String,
    b: String,
}

impl Pair {
    fn get_a(&self) -> &String {
        &self.a
    }
}

fn len_opt(s: &String) -> Option<usize> {
    s.chars().next().map(|_| s.len())
}

fn run(mut s: String) -> Option<usize> {
    let p = Pair { a: s, b: String::from("x") };
    let r = &p.b;
    if r.is_empty() {
    } else {
        let n = len_opt(p.get_a())?;
        let n2 = len_opt(r)?;
    }
    Some(0)
}
