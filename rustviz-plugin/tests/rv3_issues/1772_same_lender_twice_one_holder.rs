struct H<'a> { r: &'a String }
fn twice() {
    let mut a = String::from("a");
    let mut h = H { r: &a };
    println!("{}", h.r);
    a.push('x');
    h = H { r: &a };
    println!("{}", h.r);
}
