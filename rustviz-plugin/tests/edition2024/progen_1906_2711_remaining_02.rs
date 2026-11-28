struct Pair {
    a: String,
    b: String,
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn len_opt(s: &String) -> Option<usize> {
    s.chars().next().map(|_| s.len())
}

fn run(mut s: String) -> Option<usize> {
    let s2 = s.clone();
    let tp = (String::from("hey"), 1);
    if s2.len() > 1 {
    } else if s.len() > 1 {
        let n = len_opt(longest(&s, &s2))?;
        let p = Pair { a: s, b: tp.0 };
    }
    Some(0)
}
