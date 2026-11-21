// #1188: re-pointing a reference in a conditional written on one line.
// When `c` is false r still points to a and is read after the `if`, so a
// stays borrowed until that read, as for the same `if` over several
// lines. Main ended a's borrow at the `if` (a one-line `if`: its
// condition, on the write's line, counted as the write), or never ended
// it after the join (a bare-expression match arm).
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut r = &a;
    let c = true;
    if c { r = &b; }
    println!("{}", r);
    println!("end");
}

fn arm() {
    let d = String::from("d");
    let e = String::from("e");
    let mut q = &d;
    let n = 3;
    match n {
        1 => q = &e,
        _ => {}
    }
    println!("{}", q);
    println!("end");
}

// A `&mut`.
fn mutable() {
    let mut f = String::from("f");
    let mut g = String::from("g");
    let mut m = &mut f;
    let c = true;
    if c { m = &mut g; }
    m.push('!');
    println!("{}", f);
}

// `if let`, and a one-line `if` in a loop body.
fn if_let(o: Option<String>) {
    let h = String::from("h");
    let mut t = &h;
    if let Some(x) = &o { t = x; }
    println!("{}", t);
    println!("end");
}

fn in_loop(v: Vec<String>) {
    let k = String::from("k");
    let mut u = &k;
    for s in &v { if s.len() > 5 { u = s; } }
    println!("{}", u);
    println!("end");
}
