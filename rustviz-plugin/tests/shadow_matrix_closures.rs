// Shadowing matrix, part 4: closures and shadowing.

// A closure parameter named like an outer variable doesn't touch it.
fn closure_param() {
    let s = String::from("a");
    let f = |s: &str| s.len();
    println!("{}", f("xyz"));
    println!("{}", s);
}

// A closure captures the second (Copy) `c`.
fn captures_shadowing() {
    let c = String::from("a");
    let c = c.len();
    let f = || c + 1;
    println!("{}", f());
}

// A `move` closure takes the second `m`; the first is still borrowed.
fn move_after_shadow() {
    let m = String::from("a");
    let t = &m;
    let m = String::from("b");
    let f = move || m.len();
    println!("{} {}", f(), t);
}

// A closure borrows the first `b` before it's shadowed.
fn captures_first() {
    let b = String::from("a");
    let f = || b.len();
    let b = String::from("b");
    println!("{} {}", f(), b);
}

// Calling the second of two `FnOnce` closures named `g` moves it.
fn fnonce_shadowed() {
    let a = String::from("a");
    let g = move || a;
    let b = String::from("b");
    let g = move || b;
    let t = g();
    println!("{}", t);
}

// A `move` closure passed straight to a call takes the second `h`.
fn apply<F: FnOnce() -> usize>(f: F) -> usize {
    f()
}
fn closure_arg() {
    let h = String::from("a");
    let h = h + "b";
    let n = apply(move || h.len());
    println!("{}", n);
}

fn main() {
    closure_param();
    captures_shadowing();
    move_after_shadow();
    captures_first();
    fnonce_shadowed();
    closure_arg();
}
