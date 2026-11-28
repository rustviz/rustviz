fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn run(m: &mut String, o: Option<String>, mut v: Vec<String>, flag: bool) -> Option<usize> {
    let s = m.clone();
    let mut s2 = s;
    let mut b = Box::new(format!("{}!", m));
    let o2 = Some(String::from("x"));
    let s3 = std::mem::replace(&mut s2, String::new());
    println!("{}", s2);
    if let Some(x) = v.first() {
        let m2 = m;
        let m3 = &mut s2;
    }
    let mut o3 = maybe(String::from("abc"));
    Some(0)
}
