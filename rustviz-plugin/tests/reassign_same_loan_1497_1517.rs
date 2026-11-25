// A reference given back the reference it already holds (`r = r;`, or `a =
// b;` with b a copy of a) keeps the same loan: the write ends none of it, and
// the lender is given back once, at the last use (#1497, #1517). It used to
// end the borrow at the write ("r's immutable borrow ends"), and for a
// downgraded `&mut` say both "immutable" and "mutable borrow ends".

fn self_assign() {
    let s = String::from("ab");
    let mut r = &s;
    r = r;
    println!("{}", r);
}

// In an arm.
fn self_assign_in_arm() {
    let s2 = String::from("ab");
    let mut r2 = &s2;
    if s2.len() > 1 {
        r2 = r2;
    }
    println!("{}", r2);
}

fn downgrade(s: &mut String) -> &String {
    s
}

// A shared reference made from a `&mut` loan, copied and copied back.
fn downgraded() {
    let mut s3 = String::from("hi");
    let mut a = downgrade(&mut s3);
    let b = a;
    a = b;
}

struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

// The generator's shape: a struct field given back a copy of itself.
fn field() {
    let mut s4 = String::from("hi");
    let mut h = holder(downgrade(&mut s4));
    let Holder { r: r4, .. } = h;
    h.r = r4;
    let _ = h.n;
}

fn main() {
    self_assign();
    self_assign_in_arm();
    downgraded();
    field();
}
