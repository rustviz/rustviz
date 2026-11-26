// #1627: a closure that uses the `&mut` reference r itself
// (`println!("{}", r)`) and writes through it (`*r += 1`) captures r by a
// unique borrow: nothing else may use r, not even to read it, while the
// closure lives (adding `println!("{}", r);` before `f();` is E0501).
fn apply(f: impl FnOnce()) {
    f();
}

fn main() {
    let mut n = 0;
    let r = &mut n;
    let mut f = || {
        println!("{}", r);
        *r += 1;
    };
    f();
    apply(|| {
        println!("{}", r);
        *r += 1;
    });
    println!("{}", n);
}
