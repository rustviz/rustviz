// Known bug, from the closed 2.x fix PRs (cx/hedge-f3); kept as a RustViz 3 regression case.
// Issue(s): #1637 (see known_bugs/README.md for what main draws wrong).
struct D<'a>(&'a mut i32);
impl Drop for D<'_> {
    fn drop(&mut self) { *self.0 += 1; }
}
fn main() {
    let mut n = 0;
    let d = D(&mut n);
    if std::hint::black_box(true) {
        println!("leaving");
        return;
    }
    drop(d);
}

// The residual path drops the holder at the question mark. On success
// the call consumes it, and n2 is available again immediately afterwards.
fn with_try(opt: Option<()>) -> Option<i32> {
    let mut n2 = 0;
    let d2 = D(&mut n2);
    opt?;
    drop(d2);
    Some(n2)
}

// rustc can share the destructor block between two explicit returns.
fn both_returns(c: bool) {
    let mut value = 0;
    let guard = D(&mut value);
    if c {
        println!("one");
        return;
    } else {
        println!("two");
        return;
    }
}
