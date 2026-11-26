fn run(f: impl FnOnce() -> String) -> String { f() }
fn main() {
    let s = String::from("x");
    let out = run(move || s);
    drop(out);
}
