// `c` is overwritten before it is read on the path through the `if`, so
// x's loan is dead there (`x += 1;` compiles; with the write on the
// `if`'s condition, `if { x += 1; true } { .. }`, rustc gives E0506).
// The loan lives on only on the path that skips the `if` (#770).
fn main() {
    let mut x = 8;
    let y = 9;
    let mut c = &x;
    if true {
        x += 1;
        c = &y;
    }
    println!("{}", c);
}
