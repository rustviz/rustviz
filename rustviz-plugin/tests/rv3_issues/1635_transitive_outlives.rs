fn hold<'a: 'b, 'b, T: std::fmt::Display + 'a>(x: T) -> Box<dyn std::fmt::Display + 'b> {
    Box::new(x)
}
fn main() {
    let mut a = 1;
    let g = hold(&a);
    println!("{g}");
}
