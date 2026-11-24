// #1393 with `&mut`: `m = if c { let w = &mut s; w } else { &mut t };`
// reborrows through w into m in its branch, before w goes out of scope.
// m gives s and t back at its last use, line 10 (#1431).
fn main() {
    let mut s = String::from("s");
    let mut t = String::from("t");
    let mut m = &mut t;
    m.push('!');
    m = if s.len() > 0 { let w = &mut s; w } else { &mut t };
    m.push('?');
}
