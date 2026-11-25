// #1496: h.r whose holder is last used in one arm.
struct Holder<'a> {
    r: &'a String,
    n: i32,
}
fn main() {
    let s = String::from("abc");
    let h = Holder { r: &s, n: 0 };
    let r = &s;
    if s.len() > 2 {
        println!("{} {}", r, h.n);
    } else {
    }
}
fn else_if() {
    let s = String::from("abc");
    let h = Holder { r: &s, n: 0 };
    let r = &s;
    if s.len() > 5 {
    } else if s.len() > 2 {
        println!("{} {}", r, h.n);
    } else {
    }
}
