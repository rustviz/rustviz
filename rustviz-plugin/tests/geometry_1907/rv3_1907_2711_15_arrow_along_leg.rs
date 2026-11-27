fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn run(m: &mut String, mut o: Option<String>, mut v: Vec<String>, flag: bool) -> Option<usize> {
    let m3 = m;
    if flag {
        let r = v.first()?;
        if let Some(x) = &o && x.len() > 1 && let Some(y) = v.last() {
        }
    } else if flag {
        let m4 = &mut *m3;
    } else {
        match o {
            Some(x2) if x2.len() > 1 => {
            }
            Some(_) => {
            }
            None => {
                if m3.len() > 2 { return Some(v.len()); }
            }
        }
        let r2 = &*m3;
    }
    Some(0)
}
