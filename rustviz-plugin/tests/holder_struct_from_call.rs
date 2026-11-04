struct Excerpt<'a> {
    part: &'a str,
}
impl<'a> Excerpt<'a> {
    fn new(s: &'a str) -> Excerpt<'a> { Excerpt { part: s } }
}
fn main() {
    let novel = String::from("Call me. Some years");
    let ex = Excerpt::new(&novel);
    println!("{}", ex.part);
}
