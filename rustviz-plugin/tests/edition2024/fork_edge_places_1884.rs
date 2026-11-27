struct Holder<'a> {
    r: &'a String,
    owned: String,
}

fn see(_: &String) {}
fn test(_: &String) -> bool { true }

fn partial(s: &mut String, choose: bool) {
    let h = Holder { r: &*s, owned: String::new() };
    let q = h.r;
    drop(h.owned);
    if choose {
        see(q);
    } else {
        see(h.r);
    }
    s.clear();
}

fn joined(s: &mut String, t: &String, choose: bool, flag: bool) {
    let r = &*s;
    let mut q = r;
    if choose && { q = t; flag } {
        see(r);
    }
    see(q);
}

fn elements(a: [Option<&String>; 2], s: &mut String) {
    let r = &*s;
    if let [Some(_), _] = a && test(r) {}
    let q = &*s;
    if let [_, Some(_)] = a && test(q) {}
}

fn raw(s: &mut String, replacement: &String) {
    let h = Holder { r: &*s, owned: String::new() };
    let mut p = unsafe { &(*std::ptr::from_ref(&h)).owned };
    see(h.r);
    s.clear();
    see(p);
    p = replacement;
    see(p);
}

fn nested_lenders(a: String, b: String, c: String, t: String, outer: bool, inner: bool, change: bool) {
    let mut r = if outer {
        if inner { &a } else { &b }
    } else {
        &c
    };
    if change {
        r = &t;
        see(r);
    } else {
        see(r);
    }
}
fn main() {}
