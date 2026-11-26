fn run(v: Vec<String>, c: bool, e: bool, s2: String) {
    let other = v.first();
    if e {
        if let Some(x) = v.get(1) {
            if c {
                drop(s2);
                let o = x.clone();
            }
        }
    } else {
        v.len();
    }
    other.unwrap();
}

fn direct(v17: Vec<String>, c: bool, s17: String) {
    let other17 = &v17[0];
    if let Some(x17) = v17.first() {
        if c {
            drop(s17);
            x17.clone();
        }
    }
    other17.len();
}

fn conditional(mixed_v: Vec<String>, choose: bool, e: bool, c: bool) {
    let maybe = if choose { mixed_v.first() } else { None };
    if e {
        if let Some(mixed_x) = mixed_v.get(1) {
            if c {
                mixed_x.clone();
            }
        }
    } else {
        let n = 0;
    }
    maybe.unwrap();
}
