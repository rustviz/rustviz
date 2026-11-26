use std::fmt::Display;
fn boxed<'b, T: Display + 'b>(_t: T) -> Box<dyn Display + 'b> { Box::new(0) }
fn main() {
    let a = 1;
    let b = boxed(&a);
    println!("{b}");
}
