fn make() -> String { String::new() }
fn main() {
    let mut s = make(); s = make(); s = make(); s = make(); s = make();
    println!("{}", s);
}
