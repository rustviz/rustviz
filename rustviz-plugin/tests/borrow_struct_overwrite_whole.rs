// #538, #1676: the old value is dropped on a whole-struct overwrite.
// An unread reference field's loan can already have ended at initialization.
// A direct field overwrite can leave an old loan live (#531).
struct H<'a> { r: &'a String }
struct M<'a> { r: &'a mut String }
struct P<'a> { a: &'a String, b: &'a String }

fn main() {
    let mut s = String::from("s");
    let t = String::from("t");
    let mut hold = H { r: &s };
    hold = H { r: &t };
    let m = &mut s;
    m.push('!');
    println!("{}", hold.r);
}

// The old value is used before it is overwritten: the borrow ends at
// that last use, not at the overwrite.
fn used_before() {
    let a = String::from("a");
    let b = String::from("b");
    let mut hold = H { r: &a };
    println!("{}", hold.r);
    hold = H { r: &b };
    println!("{}", hold.r);
}

// A `&mut` field: the old exclusive borrow of u ends at its last use,
// so `u` can be used (read here) before the last use of `h`.
fn mutable() {
    let mut u = String::from("u");
    let mut v = String::from("v");
    let mut h = M { r: &mut u };
    h.r.push('1');
    h = M { r: &mut v };
    println!("{}", u);
    h.r.push('2');
}

// Two reference fields: both unread loans end on the initialization row.
fn two_fields() {
    let x = String::from("x");
    let y = String::from("y");
    let z = String::from("z");
    let mut p = P { a: &x, b: &y };
    p = P { a: &z, b: &z };
    println!("{} {}", p.a, p.b);
}
