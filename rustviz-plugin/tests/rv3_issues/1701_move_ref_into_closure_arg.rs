fn invoke<F: FnOnce() -> &'static mut String>(f: F) -> &'static mut String { f() }
fn main() {
    let r: &'static mut String = Box::leak(Box::new(String::new()));
    let back = invoke(move || r);
    back.push('x');
}
