// #1510: a shared reference holding a mutable loan (`&mut s` coerced to
// `&String`, or `downgrade(&mut s)`) passed to a call that returns a
// borrow of it. The result reborrows through r and carries r's mutable
// loan: s stays mutably borrowed until the result's last use, and there
// is no separate immutable loan of s.
struct Holder<'a> {
    r: &'a String,
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn id(s: &String) -> &String {
    s
}

fn holder(s: &String) -> Holder<'_> {
    Holder { r: s }
}

fn coerced() {
    let mut s = String::from("x");
    let r: &String = &mut s;
    let r2 = id(r);
    println!("{}", r2);
}

fn downgraded_holder() {
    let mut s = String::from("x");
    let r = downgrade(&mut s);
    let h = holder(r);
    println!("{}", h.r);
}

fn result_ends_first() {
    let mut s = String::from("x");
    let r = downgrade(&mut s);
    let r2 = id(r);
    println!("{}", r2);
    println!("{}", r);
    s.push('a');
}

fn assigned_later() {
    let mut s = String::from("x");
    let r: &String = &mut s;
    let r2;
    r2 = id(r);
    println!("{}", r2);
    s.push('a');
}

fn method_receiver() {
    let mut s = String::from("x");
    let r = downgrade(&mut s);
    let r3 = r.as_str();
    println!("{}", r3);
    s.push('a');
}

fn main() {
    coerced();
    downgraded_holder();
    result_ends_first();
    assigned_later();
    method_receiver();
}
