fn f(o: Option<i32>) {
    let s = String::new();
    let Some(x) = o else { return };
    println!("{}{}", x, s);
}
fn main() {}
