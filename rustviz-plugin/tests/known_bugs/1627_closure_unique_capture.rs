// Known bug, from the closed 2.x fix PRs (fix/a-calls-1620-1645); kept as a RustViz 3 regression case.
// Issue(s): #1627 (see known_bugs/README.md for what main draws wrong).
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
