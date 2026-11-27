fn len_of(s: &String) -> usize {
    s.len()
}

fn run(r: &String, m: &mut String, flag: bool) -> Option<usize> {
    let r2 = &*m;
    let mut r3 = r;
    if flag {
        if r3.is_empty() { } else if flag { } else { len_of(r2); }
    } else {
        let mut r4 = &*m;
    }
    Some(0)
}
