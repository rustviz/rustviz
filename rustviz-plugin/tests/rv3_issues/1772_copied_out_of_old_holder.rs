struct H<'a> { r: &'a String }
fn copied() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut h = H { r: &a };
    let x = h.r;
    h = H { r: &b };
    println!("{} {}", x, h.r);
}
