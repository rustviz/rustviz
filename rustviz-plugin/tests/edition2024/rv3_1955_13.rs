fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn split(s: String) -> (String, i32) {
    (s, 1)
}

fn run(s: String, mut v: Vec<String>) -> Option<usize> {
    let mut s2 = String::from("ab");
    let s3 = String::from("hi");
    let tp = split(s3);
    if s2.len() > 1 {
    } else {
        let m = v.last_mut()?;
        if let Some(x) = v.first() && v.len() > 1 {
            s2 = tp.0;
        }
        let s5 = s;
    }
    Some(0)
}
