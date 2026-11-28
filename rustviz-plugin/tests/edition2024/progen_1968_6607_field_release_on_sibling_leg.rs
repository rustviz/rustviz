struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let s = String::from("abc");
    let s2 = String::from("hey");
    let mut h = Holder { r: &s2, n: 1 };
    if s.len() > 1 {
        h.r = longest(&s2, &s);
    } else if s.len() > 1 {
        let s3 = s;
        println!("{} {}", s3, h.n);
    }
}
