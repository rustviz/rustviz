// #1415: a reborrow of a reborrow (`m2 = &mut *m; r2 = &*m2;`) gives its
// loans back innermost first: r2 gives m2 its loan back, then m2's borrow
// ends and m gets its loan back. A call or a struct given `&*m2` borrows
// through m2, not through m, which m2 still holds.
fn param(m: &mut String) {
    let m2 = &mut *m;
    let _r2 = &*m2;
}

struct Holder<'a> {
    r: &'a String,
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s }
}

fn id(s: &String) -> &String {
    s
}

fn in_literal(m: &mut String) -> usize {
    let m2 = &mut *m;
    let _h = Holder { r: &*m2 };
    m.len()
}

fn from_call_holder(m: &mut String) -> usize {
    let m2 = &mut *m;
    let h = holder(&*m2);
    println!("{}", h.r);
    m.len()
}

fn from_call_ref(m: &mut String) -> usize {
    let m2 = &mut *m;
    let r = id(&*m2);
    println!("{}", r);
    m.len()
}

fn in_some(m: &mut String) -> usize {
    let m2 = &mut *m;
    let o = Some(&*m2);
    println!("{:?}", o);
    m.len()
}

fn method(m: &mut String) -> usize {
    let m2 = &mut *m;
    let r = m2.as_str();
    println!("{}", r);
    m.len()
}

fn main() {
    let mut s = String::from("x");
    let m = &mut s;
    let m2 = &mut *m;
    let m3 = &mut *m2;
    let r = &*m3;
    println!("{}", r);
    m.push('!');
    println!("{}", s);
    param(&mut s);
    in_literal(&mut s);
    from_call_holder(&mut s);
    from_call_ref(&mut s);
    in_some(&mut s);
    method(&mut s);
}
