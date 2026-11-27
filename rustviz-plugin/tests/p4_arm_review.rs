fn match_condition(n: u8, a: bool, x: String) {
    if match n { 0 => a, _ => false } { drop(x); }
}
fn if_condition(a: bool, b: bool, x: String) {
    if if a { b } else { false } { drop(x); }
}
fn and_condition(a: bool, b: bool, x: String, y: String) {
    if a && b { drop(x); } else { drop(y); }
}
fn alternatives(n: u8, x: String) {
    match n { 1 | 3 => drop(x), _ => {} }
}
fn compared(s: String, c: bool) {
    let r = s.as_str();
    match r {
        "a" if c => {},
        _ => {}
    }
}
fn bindings(mut a: Vec<u32>, mut b: Vec<u32>) {
    match (a.first_mut(), b.first_mut()) {
        (Some(r), _) => { *r += 1; }
        (None, Some(r)) => { *r += 2; }
        _ => {}
    }
}
struct Held<'a>(Option<&'a i32>);
impl Drop for Held<'_> {
    fn drop(&mut self) { let _ = self.0; }
}
fn keep<'a>(f: impl FnOnce() -> &'a i32) -> Held<'a> { Held(Some(f())) }
fn reassigned(x: i32, y: i32) {
    let mut held = keep(|| &x);
    held = keep(|| &y);
    drop(held);
}
fn maybe<'a>(f: impl FnOnce() -> &'a i32) -> Option<&'a i32> { Some(f()) }
fn repeated(n: u8, x: i32) -> bool {
    match n {
        0 | 1 if { let held = maybe(|| &x); held.is_some() } => true,
        _ => false,
    }
}
fn guarded_move(v: Option<String>, c: bool, other: String) {
    match v { Some(s) if c => drop(s), _ => drop(other) }
}
