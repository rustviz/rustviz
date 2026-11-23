// #1354: borrows into struct fields. Several holders give one lender back
// on one row (each on a track of its own, no shaft along another), and a
// struct literal whose two fields borrow from one lender (the returns
// keep clear of the dots they pass).
struct Part<'p> { text: &'p str }

fn part<'a>(x: &'a str) -> Part<'a> {
    Part { text: x }
}

fn three_holders() {
    let s1 = String::from("hi");
    let p = part(&s1);
    let q = part(&s1);
    let r = part(&s1);
    println!("{} {} {}", p.text, q.text, r.text);
}

struct Two<'p> { a: &'p str, b: &'p str }

fn two_fields_one_lender() {
    let s1 = String::from("hi");
    let d = Two { a: &s1, b: &s1 };
    let e = Two { a: &s1, b: &s1 };
    println!("{} {} {} {}", d.a, d.b, e.a, e.b);
}

fn main() {
    three_holders();
    two_fields_one_lender();
}
