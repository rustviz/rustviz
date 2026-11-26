fn run(mut f: impl FnMut() -> String) -> String { f() }
fn main() {
    let mut s = String::from("x");
    let out = run(move || std::mem::take(&mut s));
    drop(out);
}
