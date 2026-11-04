// By-reference captures of a closure passed straight to a call (#298).
// The borrow lasts as long as the closure: drawn when the closure is
// gone by the end of the call, not drawn when the callee hands it
// back (`keep`), since its end isn't at the call.
fn apply<F: Fn(i32) -> i32>(f: F) -> i32 {
    f(1)
}

fn run<F: FnMut()>(mut f: F) {
    f();
    f();
}

fn keep<F: Fn() -> usize>(f: F) -> F {
    f
}

fn main() {
    let k = 10;
    let r = apply(|x| x + k);
    let mut count = 0;
    run(|| count += 1);
    let o = Some(3);
    let m = o.map(|x| x + k);
    let s = String::from("hi");
    let g = keep(|| s.len());
    println!("{} {} {:?} {}", r, count, m, g());
    println!("{}", s);
}
