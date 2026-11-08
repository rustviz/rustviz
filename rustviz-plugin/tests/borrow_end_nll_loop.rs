// A borrow ends where rustc's borrow checker ends it (#739, #745).
//
// A `for` loop's iterator holds the loan its head made until the loop
// ends, so the lender is still borrowed after the binding's last use:
// `m.len()` where the comment is would be E0499 / E0502.
//
// A reference that is given a new value is dead until then, so its old
// loan ends at its last use: `a = 3;` before `r = &b;` compiles.

fn mut_loop() {
    let mut m = vec![String::from("a")];
    for s in &mut m {
        s.push('!');
        // m is still mutably borrowed here
    } // end of mut_loop's for
    println!("{:?}", m);
}

fn shared_loop() {
    let pairs = vec![(1, 2)];
    for (a, b) in &pairs {
        println!("{} {}", a, b);
        // pairs is still borrowed here
    } // end of shared_loop's for
    println!("{:?}", pairs);
}

fn reassigned() {
    let mut a = 1;
    let b = 2;
    let mut r = &a;
    println!("{}", r);
    // a is free here: r is overwritten before it is used again
    r = &b;
    println!("{} {}", r, a);
}

// c's loan, made through `*z = &c`, ends after its last use through z.
fn through_deref() {
    let x = 7;
    let mut c = 8;
    let mut y = &x;
    let z = &mut y;
    *z = &c;
    println!("c value {}", **z);
    // c is free here
    y = &x;
    println!("x value {}", *y);
    c += 1;
}

fn main() {
    mut_loop();
    shared_loop();
    reassigned();
    through_deref();
}
