// #1622: a `move` closure passed to a function. When its body moves a
// captured value out (`move || s` returns it), calling the closure takes
// the value out again: where it's dropped depends on what run does with
// the closure (here it returns it, and `out` drops it). A body that only
// reads its capture leaves it in the closure, dropped with it.
fn run(f: impl FnOnce() -> String) -> String {
    f()
}

fn keep(f: impl Fn() -> usize) -> usize {
    f()
}

fn main() {
    let s = String::from("x");
    let out = run(move || s);
    drop(out);
    let t = String::from("y");
    let n = keep(move || t.len());
    let u = String::from("z");
    let e = run(move || {
        drop(u);
        String::new()
    });
    println!("{} {}", n, e);
}
