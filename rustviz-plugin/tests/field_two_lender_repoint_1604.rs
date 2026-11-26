// A reference field that may point into s or t (`longest`'s result),
// re-pointed in straight-line code: its old loan ends once, on one dot,
// before the new loan starts (#1604).

struct Holder<'a> {
    r: &'a String,
}

struct Pair<'a>(&'a String);

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

// The issue's program: re-pointed to t, one of its old lenders.
fn to_second_lender(s: String, t: String) {
    let mut h = Holder { r: longest(&s, &t) };
    let n = 1;
    h.r = &t;
}

// Re-pointed to s, the other one.
fn to_first_lender(s: String, t: String) {
    let mut h = Holder { r: longest(&s, &t) };
    let n = 1;
    h.r = &s;
}

// A tuple struct's field.
fn tuple_field(s: String, t: String) {
    let mut h = Pair(longest(&s, &t));
    let n = 1;
    h.0 = &t;
}

// One lender, re-pointed to the same one.
fn same_lender(s: String) {
    let mut h = Holder { r: &s };
    let n = 1;
    h.r = &s;
}

// Already right: re-pointed to a third variable.
fn to_third(s: String, t: String, u: String) {
    let mut h = Holder { r: longest(&s, &t) };
    let n = 1;
    h.r = &u;
}

struct HolderMut<'a> {
    r: &'a mut String,
}

fn pick<'a>(a: &'a mut String, b: &'a mut String) -> &'a mut String {
    if a.len() > b.len() { a } else { b }
}

// A `&mut` field that may point into s or t, re-pointed to u.
fn to_third_mut(mut s: String, mut t: String, mut u: String) {
    let mut h = HolderMut { r: pick(&mut s, &mut t) };
    let n = 1;
    h.r = &mut u;
}

// Read after the write: h's one lifetime keeps s and t borrowed to the read
// (#531).
fn to_third_mut_read(mut s: String, mut t: String, mut u: String) {
    let mut h = HolderMut { r: pick(&mut s, &mut t) };
    let n = 1;
    h.r = &mut u;
    h.r.push('x');
}

// One right-hand-side call borrowing both lenders again.
fn call_rhs(s: String, t: String) {
    let mut h = Holder { r: longest(&s, &t) };
    let n = 1;
    h.r = longest(&t, &s);
}

fn main() {
    to_second_lender(String::from("a"), String::from("b"));
    to_first_lender(String::from("a"), String::from("b"));
    tuple_field(String::from("a"), String::from("b"));
    same_lender(String::from("a"));
    to_third(String::from("a"), String::from("b"), String::from("c"));
    to_third_mut(String::from("a"), String::from("b"), String::from("c"));
    to_third_mut_read(String::from("a"), String::from("b"), String::from("c"));
    call_rhs(String::from("a"), String::from("b"));
}
