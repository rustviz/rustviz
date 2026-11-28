struct Pair {
    a: String,
    b: String,
}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn run(m: &mut String) -> Option<usize> {
    let p = Pair { a: String::from("abc"), b: std::mem::replace(m, String::new()) };
    let v: Vec<String> = Vec::new();
    let o = Some(String::from("hi"));
    let Pair { b: r2, .. } = &p;
    if m.len() > 2 {
    } else {
        let r5 = v.first()?;
        println!("{} {:?}", r2, o);
    }
    Some(0)
}
