struct H<'a> { r: &'a String }
pub fn looped(n: usize) {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut hold = H { r: &a };
    for _ in 0..n {
        hold = H { r: &b };
    }
    println!("{}", hold.r);
}
