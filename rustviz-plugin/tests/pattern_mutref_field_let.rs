struct Part<'a> { part: &'a mut String }
fn main() {
    let mut s = String::from("hello");
    let p = Part { part: &mut s };
    let Part { part: r } = p;
    r.push('x');
    println!("{}", s);
}
