// Known bug, from the closed 2.x fix PRs (cx/cx-c09); kept as a RustViz 3 regression case.
// Issue(s): #1791 (see known_bugs/README.md for what main draws wrong).
fn f(c: bool, stop: bool) {
    let mut s = String::from("a");
    if c { drop(s); }
    let _ = stop && { s = String::new(); true };
    println!("end");
}

fn or_in_arg(c: bool, stop: bool) {
    let mut t = String::from("a");
    if c { drop(t); }
    std::hint::black_box(stop || { t = String::new(); false });
    println!("end");
}

fn closure_rhs(c: bool, next: Option<bool>) {
    let mut x = String::new();
    if c { drop(x); }
    if let Some(true) = next { x = (|| String::new())(); }
}

fn main() {}
