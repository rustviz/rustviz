fn make() -> String {
    String::from("made")
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn run(r: &String, flag: bool) -> Option<usize> {
    let s = make();
    let mut o = maybe(s);
    if r.is_empty() {
        let m = o.as_mut().unwrap();
    } else if flag {
        let s2 = match o {
            Some(y) if y.len() > 1 => y,
            Some(_) => String::new(),
            None => String::new(),
        };
    }
    Some(0)
}
