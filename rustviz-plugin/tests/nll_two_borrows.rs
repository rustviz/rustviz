// Non-lexical lifetimes: the first &mut borrow ends at its last use,
// so a second &mut borrow on the next line is allowed.

fn world(s: &mut String) {
    s.push_str(", world");
}

fn main() {
    let mut x = String::from("Hello");
    let y = &mut x;
    world(y);
    let z = &mut x; // OK: y's lifetime ends after world(y)
    world(z);
    println!("{}", x);
}
