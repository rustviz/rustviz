// #1180: `|| println!("{}", r)` captures the reference variable r itself
// (the macro takes `&r`), not what r points to; `|| r.len()` captures `*r`.
fn apply<F: Fn()>(f: F) { f(); }
fn main() {
    let s = String::from("hello");
    let r = &s;
    let show = || println!("{}", r);
    show();
    let len = || r.len();
    len();
    apply(|| println!("{}", r));
}
