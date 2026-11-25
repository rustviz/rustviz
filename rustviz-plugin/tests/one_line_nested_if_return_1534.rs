// #1534: a one-line conditional whose arm holds a one-line `if` that
// returns, its condition reading s2. The inner fork (where is_empty
// borrows s2) was drawn above the outer one, and the outer leg into the
// arm ended in mid-air in the inner diamond. In the `else` arm, or with
// an empty `else`, the two diamonds' forks or joins shared a row and
// their legs crossed. The inner fork now sits on the outer leg below the
// outer fork, and the outer join below the inner one.
fn then_arm(c: bool) {
    let s2 = String::from("hi");
    if c { if s2.is_empty() { return; } } else { s2.len(); }
}
fn else_arm(c: bool) {
    let s2 = String::from("hi");
    if c { s2.len(); } else { if s2.is_empty() { return; } }
}
fn empty_else(c: bool) {
    let s2 = String::from("hi");
    if c { if s2.is_empty() { return; } } else { }
}
fn moved_in_inner(c: bool) {
    let s2 = String::from("hi");
    if c { if s2.is_empty() { drop(s2); } } else { s2.len(); }
}
fn match_arm(n: u8) {
    let s2 = String::from("hi");
    match n { 0 => if s2.is_empty() { return; }, _ => { s2.len(); } }
}
fn if_let(o: Option<i32>) {
    let s2 = String::from("hi");
    if let Some(_x) = o { if s2.is_empty() { return; } } else { s2.len(); }
}
fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}
// The generator's program (seed 404, bucket 12).
fn generated() -> Option<usize> {
    let s = String::from("hi");
    let s2 = s.to_string();
    if s.len() > 1 { if s2.len() > 1 { return None; } } else { let _r3 = longest(&s2, &s2); }
    Some(0)
}
fn main() {
    then_arm(true);
    else_arm(true);
    empty_else(true);
    moved_in_inner(true);
    match_arm(0);
    if_let(Some(1));
    generated();
}
