// #1279: a reference whose last use is a later part of a let-chain
// condition (`if let Some(x) = o && bump(r)`). That part runs only on the
// way into the `if` arm, so the borrow ends there, after the use; on the
// path into the else arm it has already ended. (A condition over several
// lines is #1291.)
fn bump(r: &mut i32) -> bool { *r += 1; *r > 2 }

// A reference parameter.
fn chain_param(r1: &mut i32, o: Option<i32>) {
    if let Some(x) = o && bump(r1) {
        println!("{}", x);
    } else {
        println!("no");
    }
}

// A local `&mut`: its lender a gets its value back in each arm.
fn chain_local(o: Option<i32>) {
    let mut a = 0;
    let r2 = &mut a;
    if let Some(x) = o && bump(r2) {
        println!("{}", x);
    } else {
        println!("no");
    }
    println!("{}", a);
}

// A shared reference read in the chain.
fn chain_shared(o: Option<i32>) {
    let b = 5;
    let r3 = &b;
    if let Some(x) = o && *r3 > x {
        println!("{}", x);
    } else {
        println!("no");
    }
    println!("{}", b);
}

// Two `let` parts before the use.
fn chain_two_lets(o: Option<i32>, p: Option<i32>) {
    let mut d = 0;
    let r5 = &mut d;
    if let Some(x) = o && let Some(y) = p && bump(r5) {
        println!("{} {}", x, y);
    } else {
        println!("no");
    }
    println!("{}", d);
}

// In an `else if`.
fn chain_else_if(go: bool, o: Option<i32>) {
    let mut e = 0;
    let r6 = &mut e;
    if go {
        println!("go");
    } else if let Some(x) = o && bump(r6) {
        println!("{}", x);
    } else {
        println!("no");
    }
    println!("{}", e);
}

fn main() {
    let mut n = 0;
    chain_param(&mut n, Some(1));
    chain_local(Some(2));
    chain_shared(None);
    chain_two_lets(Some(1), Some(2));
    chain_else_if(false, Some(4));
}
