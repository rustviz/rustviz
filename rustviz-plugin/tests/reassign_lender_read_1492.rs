// A reference re-pointed later (`r = &t;`) still borrows its old lender
// until its old loan ends. A read of the lender in between (`println!("{}",
// b)`) gives back only its own borrow: b stays shared by r (#1492). It used
// to say "b's resource is no longer immutably borrowed" and draw b as the
// owner from there.

// Straight-line: r is read after the lender, then re-pointed.
fn straight() {
    let b = String::from("b");
    let t = String::from("t");
    let mut r = &b;
    println!("{}", b);
    println!("{}", r);
    r = &t;
    println!("{}", r);
}

// The re-point in one arm: on the `else` path r still holds its loan of b2
// until the read there.
fn in_arm(c: bool) {
    let b2 = String::from("b");
    let t2 = String::from("t");
    let mut r2 = &b2;
    println!("{}", b2);
    if c {
        r2 = &t2;
        println!("{}", r2);
    } else {
        println!("{}", r2);
    }
}

// In a match arm, the lender read by a method call.
fn in_match(n: usize) {
    let b3 = String::from("b");
    let t3 = String::from("t");
    let mut r3 = &b3;
    b3.len();
    match n {
        0 => {
            r3 = &t3;
            println!("{}", r3);
        }
        _ => println!("{}", r3),
    }
}

// Two lenders, read in the `if` condition.
fn two_lenders(c: bool) {
    let a4 = String::from("a");
    let b4 = String::from("b");
    let t4 = String::from("t");
    let mut r4 = if c { &a4 } else { &b4 };
    if b4.len() > 0 {
        r4 = &t4;
        println!("{}", r4);
    } else {
        println!("{}", r4);
    }
}

fn main() {
    straight();
    in_arm(true);
    in_match(0);
    two_lenders(true);
}
