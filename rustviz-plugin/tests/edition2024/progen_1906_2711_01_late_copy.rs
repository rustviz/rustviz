fn make() -> String {
    String::from("made")
}

fn push_x(s: &mut String) {
    s.push('x');
}

fn mut_gen<T>(_t: &mut T) {}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn wrap_mut<'a>(s: &'a mut String) -> &'a mut String {
    s
}

fn run(m: &mut String, flag: bool) -> Option<usize> {
    let mut s = make();
    let v = vec![make(), make()];
    let _ = v;
    let [x, ..] = v.as_slice() else { return None; };
    let r = x;
    push_x(&mut s);
    if let Some(x2) = v.first() && m.len() > 2 && let Some(y) = v.last() {
        mut_gen(&mut s);
        let m2 = m;
        let m3 = wrap_mut(&mut s);
    } else {
        if s.len() > 1 {
            push_x(m);
            drop(s);
            let r2 = r;
        } else {
            let n = s.len();
            s = String::from("ab");
            let r3 = &s;
        }
    }
    let r4 = &v[0];
    let s2;
    if flag {
        s2 = r4.clone();
    } else {
        s2 = String::from("abc");
    }
    Some(r4.len())
}
