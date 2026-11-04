// A closure passed to a function defined in the program, whose body
// calls it: the closure's body runs during that call (#278). A function
// that never calls it (`ignore`) and one from the standard library
// (`Option::map`) get no such dot.
fn apply<F: Fn()>(f: F) {
    f();
}

fn call_twice<F: FnMut()>(mut f: F) {
    f();
    f();
}

fn ignore<F: Fn() -> usize>(_f: F) -> usize {
    0
}

fn main() {
    let s = String::from("hi");
    let show = || println!("{}", s);
    apply(&show);
    let mut count = 0;
    let mut inc = || count += 1;
    call_twice(&mut inc);
    let w = String::from("w");
    let len = || w.len();
    let z = ignore(len);
    let o = Some(1);
    let p = o.map(|x| x + len());
    println!("{} {} {:?}", count, z, p);
}
