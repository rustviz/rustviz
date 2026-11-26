struct Holder<'a> {
    r: &'a String,
}
fn main() {
    let s = String::from("hey");
    let mut h = Holder { r: &s };
    if s.len() > 1 {
        h.r = &s;
    } else {
        println!("{}", h.r);
    }
}
