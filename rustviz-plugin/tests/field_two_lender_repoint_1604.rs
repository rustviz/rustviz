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

fn main() {
    to_second_lender(String::from("a"), String::from("b"));
    to_first_lender(String::from("a"), String::from("b"));
    tuple_field(String::from("a"), String::from("b"));
    same_lender(String::from("a"));
    to_third(String::from("a"), String::from("b"), String::from("c"));
}
