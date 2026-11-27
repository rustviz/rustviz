fn see(_: &String) {}
fn test(_: &String) -> bool { true }

fn copied(s: &mut String, choose: bool) {
    let r = &*s;
    let q = r;
    if choose {
        see(r);
        s.clear();
    } else {
        see(q);
        s.clear();
    }
}

fn nested(s: &mut String, a: bool, b: bool) {
    let r = &*s;
    if a {
        if b {
            see(r);
        } else { }
    } else {
        see(r);
    }
    s.clear();
}

fn neighboring(s: &mut String, a: bool, b: bool) {
    let r = &*s;
    if a {
        see(r);
    } else { see(r); } if b {
        s.clear();
    } else { s.push('b'); }
}

fn chain(s: &mut String, flag: Option<()>) {
    let r = &*s;
    if let Some(()) = flag && test(r) {
        s.clear();
    } else {
        s.push('e');
    }
}

enum Alternative<'a> { First(&'a String), Second(&'a String), Empty }

fn shared_target(s: &mut String, n: u8) {
    let r = &*s;
    let value = match n {
        0 => Alternative::Empty,
        1 => Alternative::First(r),
        _ => Alternative::Second(r),
    };
    if let Alternative::First(p) = value {
        see(p);
    } else {
        s.clear();
    }
}

fn main() {}
