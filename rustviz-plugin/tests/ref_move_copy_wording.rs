// An inferred `let r2 = r` moves the mutable reference. Returning `r`
// to the declared reference type reborrows its referent. Copying a
// shared reference leaves the referent with its owner.
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
