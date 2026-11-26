fn run(f: impl FnOnce() -> String) -> String {
    f()
}
fn main() {
    drop(run(String::new));
}
