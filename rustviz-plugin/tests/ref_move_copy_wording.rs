// #178: moving a `&mut` (`let r2 = r;`, or returning it) moves the
// reference, and copying a `&T` copies the reference; the resource it
// points to stays with its owner `s` / `x`.
fn pass(r: &mut String) -> &mut String { r }

fn main() {
    let mut s = String::from("hi");
    let r = &mut s;
    let r2 = r;
    let r3 = pass(r2);
    r3.push('!');
    let x = 5;
    let a = &x;
    let b = a;
    println!("{} {} {}", s, a, b);
}
