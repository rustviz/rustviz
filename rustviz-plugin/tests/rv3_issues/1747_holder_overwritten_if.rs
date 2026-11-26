struct H<'a> { r: &'a String }
pub fn cond(flag: bool) {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut hold = H { r: &a };
    if flag {
        hold = H { r: &b };
    }
    println!("{}", hold.r);
}
