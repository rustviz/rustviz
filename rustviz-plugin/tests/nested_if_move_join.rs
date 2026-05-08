// Nested conditional join — issue #108. The inner `if` already
// produces a "may have been moved" join for `s` (consume on one
// inner branch, borrow on the other); the outer `if`'s else branch
// also consumes `s`, so the outer merge has to surface "may have
// been moved" too.

fn consume(_s: String) {}

fn main() {
    let s = String::from("hi");
    let c1 = true;
    let c2 = false;
    if c1 {
        if c2 {
            consume(s);
        } else {
            println!("kept inner: {}", s);
        }
    } else {
        consume(s);
    }
}
