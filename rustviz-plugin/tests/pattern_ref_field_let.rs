struct Part<'a> { part: &'a str }
fn main() {
    let s = String::from("hello");
    let p = Part { part: &s };
    let Part { part: r } = p;
    println!("{}", r);
}
