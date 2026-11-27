fn push_x(s: &mut String) {
    s.push('x');
}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn run(r: &String, m: &mut String, mut o: Option<String>, flag: bool) -> Option<usize> {
    let s = String::from("ab");
    let s2 = format!("{}!", m);
    let mut v = vec![s, r.clone()];
    v = Vec::new();
    let mut r2 = r;
    let Some(x) = v.first() else {
        while let Some(x2) = v.pop() {
            *m = s2.clone();
            let m2 = o.as_mut().unwrap();
        }
        return None;
    };
    if flag {
        r2 = v.last().unwrap();
        push_x(m);
        println!("{} {:?}", r2, v);
    } else {
        println!("{:?}", v);
        let r3 = &s2;
        if let Some(x3) = o && x3.len() > 1 {
            drop(s2);
            m.push('!');
        } else {
            let r4 = &s2;
            if let Some(l) = v.last_mut() {
                *l = String::from("abc");
            }
        }
    }
    let s3 = v.pop().unwrap();
    v = Vec::new();
    Some(m.len())
}
