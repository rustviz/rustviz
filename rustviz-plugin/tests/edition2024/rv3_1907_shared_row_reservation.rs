struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn len_opt(s: &String) -> Option<usize> {
    s.chars().next().map(|_| s.len())
}

fn run(s: String, r: &String, o: Option<String>, flag: bool) -> Option<usize> {
    let s2 = String::from("ab");
    let s3 = make();
    println!("{}", s3);
    let s4 = o?;
    println!("{}", s);
    let o2 = Some(s3.clone());
    let c = || s4.len();
    println!("{:?} {}", o2, r);
    let s4 = &s4;
    if s4.is_empty() && let Some(x) = &o2 && x.len() > 1 {
        let mut p = Pair { a: s, b: o2.unwrap() };
        if s3.len() > 1 {
            drop(p);
        } else if flag {
            let x2 = s2.as_str();
            let mut v = vec![String::from("x"), make()];
            let _ = p;
        } else {
            let mut o3: Option<String> = None;
            let mut v2 = vec![s2, s3.clone()];
        }
        let n = len_opt(&s3)?;
    }
    Some(0)
}
