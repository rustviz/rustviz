fn boxed<'a>(f: impl Fn(&i32) -> bool + 'a) -> Box<dyn Fn(&i32) -> bool + 'a> { Box::new(f) }
fn main() {
    let s = String::from("hi");
    let other = String::from("x");
    let mut r = &s;
    let m = boxed(|x| r.len() == *x as usize);
    r = &other;
    println!("{} {}", m(&2), r);
}
