struct H<'a> { r: &'a String }
fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut hold = H { r: &a };
    let other = H { r: &b };
    hold = other;
    a.push('!');
    println!("{}", hold.r);
}
