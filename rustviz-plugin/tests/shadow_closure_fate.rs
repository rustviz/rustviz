// A closure's captured variable is a shadowing one (#173): the FnOnce
// call's capture fate names `u₂` (the String the closure captured), not
// the first `u`.
fn main() {
    let u = String::from("u");
    let u = String::from("v");
    let g2 = move || drop(u);
    g2();
}
