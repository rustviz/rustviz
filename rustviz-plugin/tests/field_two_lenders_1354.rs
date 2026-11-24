// #1354: both fields of `Two<'a>` may borrow from s1 or s2. Each field
// holds one reference, so one arrow is drawn into it (from s1, the first
// lender) and one back out at its last use (to s1), and their tooltips and
// the field's dots name both lenders. Four borrows and four returns, a
// pair per field, used to overlap between s2 and d.a.
struct Two<'p> { a: &'p str, b: &'p str }

fn two<'a>(c: &'a str, d: &'a str) -> Two<'a> {
    Two { a: c, b: d }
}

fn main() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let d = two(&s1, &s2);
    println!("{} {}", d.a, d.b);
}
