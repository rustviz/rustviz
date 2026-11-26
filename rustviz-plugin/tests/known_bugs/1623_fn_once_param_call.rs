// Known bug, from the closed 2.x fix PRs (fix/a-calls-1620-1645); kept as a RustViz 3 regression case.
// Issue(s): #1623 (see known_bugs/README.md for what main draws wrong).
// #1623: calling an `impl FnOnce` parameter (or a `F: FnOnce`, or a
// `Box<dyn FnOnce>`) consumes it, and what the caller (or `x`, or `drop`)
// gets is what the call returns, not `f`. Here `f` is the function
// `String::new`, not a closure.
fn run(f: impl FnOnce() -> String) -> String {
    f()
}

fn run_let<F: FnOnce() -> String>(f: F) -> usize {
    let x = f();
    x.len()
}

fn run_boxed(f: Box<dyn FnOnce() -> String>) {
    drop(f());
}

fn main() {
    drop(run(String::new));
    let s = String::from("x");
    println!("{}", run_let(move || s));
    run_boxed(Box::new(String::new));
}
