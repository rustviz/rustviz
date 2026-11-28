fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn len_opt(s: &String) -> Option<usize> {
    s.chars().next().map(|_| s.len())
}

fn run(m: &mut String, o: Option<String>, v: Vec<String>, flag: bool) -> Option<usize> {
    let s = String::from("rust");
    let mut arr = [format!("{}!", m), String::from("rust")];
    let n = s.len();
    m.push('!');
    let n2 = len_opt(longest(&s, &s))?;
    let Some(x) = v.first() else {
        let v2 = vec![String::from("hey"), String::from("hi")];
        return None;
    };
    let s2 = if v.len() > 1 {
        let Some(x2) = o else { return None; };
        x2
    } else {
        String::from("hi")
    };
    let s3 = if n > 0 { s2.clone() } else { String::from("ab") };
    let m2 = m;
    println!("{}", n);
    Some(0)
}
