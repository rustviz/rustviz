// A closure the callee hands back (#686): the variable bound to the
// call's result holds the closure, and with it the borrow the closure
// made, so the captured variable stays borrowed until that variable's
// last use, not just during the call.
fn keep<F: Fn() -> usize>(f: F) -> F {
    f
}

fn keep_mut<F: FnMut()>(f: F) -> F {
    f
}

fn main() {
    let s = String::from("hi");
    let g = keep(|| s.len());
    println!("{}", g());
    println!("{}", s);
    let mut t = String::from("a");
    let mut h = keep_mut(|| t.push('!'));
    h();
    println!("{}", t);
    let u = String::from("u");
    let f = || u.len();
    let k = keep(f);
    println!("{}", k());
}
