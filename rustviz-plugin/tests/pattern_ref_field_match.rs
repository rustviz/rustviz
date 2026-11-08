struct Part<'a> { part: &'a str }
fn main() {
    let s = String::from("hello");
    let p = Part { part: &s };
    match p {
        Part { part: r } => println!("{}", r),
    }
}
