fn take(_: String) {}
fn make(c: bool) -> Option<String> { c.then(String::new) }
fn spin() -> ! { loop {} }

pub fn return_before_sibling(c: bool, d: bool, n: i32, s: String) -> i32 {
    if c { return n; } else if d { take(s); } else { drop(s); }
    0
}

pub fn diverge_with_scrutinee(c: bool, d: bool, s: String) {
    if let Some(ref r) = make(c) { let _ = r; spin(); } else if d { take(s); } else { drop(s); }
}
