// #1525: a borrow whose last use is an if condition, with an early exit.
fn f_1525a() {
    let n = 3;
    let mut s = String::new();
    if n > 0 {
        let r = &mut s;
        if r.len() > 2 {
            return;
        }
        if n > 1 { return; }
    }
}
fn r_1525b(c: bool) {
    let mut s = String::from("rust");
    let vr = vec![&mut s];
    if vr.is_empty() {
        println!("a");
    } else {
        if c { return; }
    }
}
fn r_1525c(c: bool) -> Option<usize> {
    let s2 = String::from("ab");
    let vr = vec![s2.as_str()];
    if vr.is_empty() {
        if c { return None; }
        drop(s2);
    } else {
        if c { drop(s2); }
    }
    Some(0)
}
fn r_1525d(c: bool) -> i32 {
    let s = String::from("hi");
    let vr = vec![s.as_str()];
    if vr.is_empty() {
    } else {
        println!("{}", s);
    }
    if c {
        return 1;
    }
    0
}
fn r_1525e(c: bool) {
    let s = String::from("a");
    let vr = vec![s.as_str()];
    if vr.is_empty() { drop(s); } else { drop(s); }
    if c {
        return;
    }
}
fn f_1525f() {
    let c = true;
    let s = String::from("hi");
    let vr = vec![s.as_str()];
    if vr.is_empty() {
    } else {
        if c { return; }
    }
}
fn f_1525g() {
    let o = Some(String::from("hey"));
    if let Some(x3) = o.as_ref() {
        for i in 0..2 {
            if x3.is_empty() {
                break;
            }
        }
    } else {
    }
}
fn f_1525h() {
    let mut s = String::from("x");
    let m = &mut s;
    let r = &*m;
    let n = if m.len() > 2 {
        s.len()
    } else {
        0
    };
}
fn f_s1525_match() {
    let mut s = String::from("x");
    let m = &mut s;
    let r = &*m;
    match m.len() {
        0 => { s.push('a'); }
        _ => {}
    }
}
fn main() {}
