fn make() -> String {
    String::from("made")
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn run(m: &mut String) -> Option<usize> {
    let s = m.to_string();
    let o = maybe(String::from("ab"));
    let m2 = m;
    let mut b = Box::new(make());
    if let Some(x) = o {
        let r2 = &*m2;
    } else {
        if let Some(x2) = o {
            b = Box::new(s);
        }
    }
    Some(0)
}
