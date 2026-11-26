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

struct Reads<'a> { r: &'a String }
impl Drop for Reads<'_> {
    fn drop(&mut self) { println!("{}", self.r); }
}

fn read_by_drop() {
    let first = String::from("first");
    let next = String::from("next");
    let mut h = Reads { r: &first };
    println!("{}", next);
    h = Reads { r: &next };
    println!("{}", h.r);
}
