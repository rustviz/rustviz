// Known bug, from the closed 2.x fix PRs (fix/a-calls-1620-1645); kept as a RustViz 3 regression case.
// Issue(s): #1627, #1622, #1701 (see known_bugs/README.md for what main draws wrong).
// #1696's review: a `&mut` moved into a closure passed to a call moves
// the reference (the String stays with its owner, #1701); a closure that
// prints a struct's `&mut` field and writes through it borrows that field
// uniquely; a closure returned by a call and Copy is copied into each call.
struct S<'a> {
    r: &'a mut i32,
}

fn keep(f: impl FnOnce() -> &'static mut String) -> &'static mut String {
    f()
}

fn pass<F: FnOnce()>(f: F) -> F {
    f
}

fn main() {
    let r: &'static mut String = Box::leak(Box::new(String::new()));
    let back = keep(move || r);
    back.push('x');
    let mut n = 0;
    let s = S { r: &mut n };
    let mut g = || {
        println!("{}", s.r);
        *s.r += 1;
    };
    g();
    let k = 1;
    let f = pass(move || println!("{k}"));
    f();
    f();
}
