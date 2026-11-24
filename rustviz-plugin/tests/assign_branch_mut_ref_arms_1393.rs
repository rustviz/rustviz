// #1393 with `&mut`: `m = if c { let w = &mut s; w } else { &mut t };`
// moves w into m in its branch, before w goes out of scope. (Where t's
// new loan ends is #1431.)
fn main() {
    let mut s = String::from("s");
    let mut t = String::from("t");
    let mut m = &mut t;
    m.push('!');
    m = if s.len() > 0 { let w = &mut s; w } else { &mut t };
    m.push('?');
}
