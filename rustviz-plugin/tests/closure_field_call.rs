// #703: the callee is an expression, not a name: a closure stored in a
// struct field (`(w.f)()`), and the closure a call returns
// (`adder(t)(1)`, where `t` is moved into `adder` first). Both used
// to crash the plugin.

struct W<F: Fn() -> usize> {
    f: F,
}

fn wrap<F: Fn() -> usize>(f: F) -> W<F> {
    W { f }
}

fn adder(s: String) -> impl Fn(usize) -> usize {
    move |k| s.len() + k
}

fn main() {
    let s = String::from("s");
    let w = wrap(|| s.len());
    let n = (w.f)();
    println!("{}", n);
    let t = String::from("t");
    let m = adder(t)(1);
    println!("{}", m);
}
