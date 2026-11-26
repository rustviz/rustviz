struct Holder<'a> { r: Option<&'a mut String> }
fn keep<'a>(h: &mut Holder<'a>, r: &'a mut String) { h.r = Some(r); }
fn main() {
    let mut s = String::new();
    let r = &mut s;
    let mut holder = Holder { r: None };
    keep(&mut holder, r);
    println!("{:?}", holder.r);
}
