struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn make_pair() -> Pair {
    Pair { a: make(), b: make() }
}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn wrap_mut<'a>(s: &'a mut String) -> &'a mut String {
    s
}

fn run(o: Option<String>, v: Vec<String>, flag: bool) -> Option<usize> {
    let mut s2 = make();
    let p = make_pair();
    let m = wrap_mut(&mut s2);
    if flag {
    } else if m.len() > 2 {
        let Some(x2) = v.first() else {
            return None;
        };
        let arr = [o.unwrap(), p.b];
    }
    Some(0)
}
