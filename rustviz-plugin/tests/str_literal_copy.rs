// #211: two return arrows on the same line (s and t both last used on
// the println! line). Their order used to depend on HashMap iteration
// order, so the SVG bytes changed from run to run.
fn main() {
    let s: &str = "hello";
    let t = s;
    println!("{} {}", s, t);
}
