fn longest<'a>(a: &'a String, b: &'a String) -> &'a String { if a.len() > b.len() { a } else { b } }
fn main() {
    let s = String::from("hi");
    let s2 = String::from("ab");
    let s3 = s;
    let r = longest(&s3, &s2);
    let r2 = r;
}
