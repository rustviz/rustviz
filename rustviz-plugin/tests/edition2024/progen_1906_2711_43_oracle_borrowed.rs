fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn run(mut o: Option<String>, v: Vec<String>) -> Option<usize> {
    if let Some(x) = v.first() && v.len() > 1 {
        let s4 = match o {
            Some(y) => y,
            None => String::new(),
        };
    }
    Some(0)
}
