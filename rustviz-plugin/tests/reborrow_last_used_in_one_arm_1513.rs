// #1513: a reference whose borrow is extended to a reborrow (or copy)
// last used in one arm ends in each arm, not on its column between the
// arms' legs.
fn param_mut(m: &mut String, b: bool) {
    let m2 = &mut *m;
    if b {
    } else {
        m2.push('a');
    }
}
fn param_shared(r: &String, b: bool) {
    let r2 = &*r;
    if b {
    } else {
        println!("{}", r2);
    }
}
fn local_copy() {
    let s = String::from("ab");
    let c = true;
    let a = &s;
    let r2 = a;
    if c {
    } else {
        println!("{}", r2);
    }
}
fn local_copy_no_else() {
    let s = String::from("ab");
    let a = &s;
    let r2 = a;
    let t;
    if s.is_empty() {
        t = r2.len();
    }
}
fn param_match(m: &mut String, n: i32) {
    let m2 = &mut *m;
    match n {
        0 => {}
        _ => m2.push('a'),
    }
}
fn param_empty_else(m: &mut String, b: bool) {
    let m2 = &mut *m;
    if b {
        m2.push('a');
    } else {
    }
}
fn main() {}
