fn nested(a: bool, b: bool, x: String, y: String) {
    if a { if b { drop(x); } else { drop(y); } } else {}
}
fn implicit(c: bool, x: String) {
    if c { drop(x); }
}
fn guarded(v: Option<String>, c: bool) {
    match v { Some(s) if c => drop(s), _ => {} }
}
fn let_else(v: Option<String>) {
    let Some(s) = v else { return };
    drop(s);
}
fn same_exit(a: bool, b: bool, s: String) {
    if a { return; } if b { return; }
    println!("{s}");
}
fn keep<'a>(f: impl FnOnce() -> &'a mut i32) -> Option<&'a mut i32> { Some(f()) }
fn closure_slots() {
    { let mut x = 1; let held = keep(|| &mut x); let _ = held.is_some(); }
    { let mut x = 2; let held = keep(|| &mut x); let _ = held.is_some(); }
}
fn let_else_empty(v: Option<String>) {
    let Some(_) = v else { return };
    drop(v);
}
