struct P { a: i32, b: String }
fn boxed<'a>(f: impl Fn() -> usize + 'a) -> Box<dyn Fn() -> usize + 'a> { Box::new(f) }
fn main() {
    let mut p = P { a: 1, b: String::new() };
    let m = boxed(|| p.b.len());
    p.a = 5;
    println!("{}", m());
}
