struct Part<'a> { part: &'a mut String }
fn main() {
    let mut s = String::from("hello");
    let p = Part { part: &mut s };
    match p {
        Part { part: r } => r.push('x'),
    }
    println!("{}", s);
}
