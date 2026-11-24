// #1446: `let Some(f) = p else { .. };` with p a value holding a borrow
// (`p = v.first()`, an `Option<&String>`): the copy (or move) out of p
// gave f p's lender with no arrow saying so, yet f gave v back at its
// last use. As for `if let Some(f) = p`, f borrows v: the reference
// stored in p.
fn copied() {
    let v = vec![String::from("a")];
    let p = v.first();
    let Some(f) = p else { return };
    println!("{} {:?}", f, v);
}

// `&mut`, moved out of p. (On the `return` path p gives v back before
// the `return` drops it, #1458.)
fn moved() {
    let mut v = vec![String::from("a")];
    let p = v.last_mut();
    let Some(f) = p else { return };
    f.push('x');
    println!("{:?}", v);
}

// In a loop, with `continue`.
fn in_loop() {
    let v = vec![String::from("a")];
    for _ in 0..2 {
        let p = v.first();
        let Some(f) = p else { continue };
        println!("{}", f);
    }
    println!("{:?}", v);
}

fn main() {
    copied();
    moved();
    in_loop();
}
