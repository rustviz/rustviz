struct Holder<'a> { r: &'a String }
fn main() {
    let t = String::from("a");
    let g = Holder { r: &t };
    if t.is_empty() {
    } else if t.len() > 1 {
        let g2 = Holder { r: g.r };
        println!("{}", g2.r);
    } else {
    }
}
