// #1287, #1298: a one-line `if` inside a let-chain's block, on the
// `if let` line. Its fork (a row above the line, like any one-line
// conditional's) was drawn above the outer fork that leads into it, the
// outer leg ran into the middle of its diamond, and below its join a
// dashed stretch ran on one row and stopped in mid-air. The outer fork is
// now above the inner one, the leg ends at the inner fork, and the arm
// stops at the inner join when the inner `if` may have moved the value.
// Also the else-arm of `if let Some(a) = y && { w = a; … }` (#1293's
// chain_assigns_moved_603): its column ran on past the dot saying y may
// have been moved.
fn take(s: String) -> bool { s.len() > 0 }
fn with_else(y: Option<i32>) {
    let s = String::from("a");
    if let Some(a) = y && { if a > 2 { take(s); } a > 3 } {
        println!("big");
    } else {
        println!("small");
    }
}
fn no_else(y: Option<i32>) {
    let s = String::from("a");
    if let Some(a) = y && { if a > 2 { take(s); } a > 3 } {
        println!("big");
    }
}
fn kept(y: Option<i32>) {
    let s = String::from("a");
    if let Some(a) = y && { if a > 2 { println!("{}", s); } a > 3 } {
        println!("{}", s);
    } else {
        println!("{}", s);
    }
}
fn if_else_inner(y: Option<i32>) {
    let s = String::from("a");
    let mut z = 0;
    if let Some(a) = y && { if a > 2 { z = a; } else { take(s); } z > 3 } {
        println!("big");
    } else {
        println!("{}", z);
    }
}
fn while_let(mut v: Vec<i32>) {
    let mut z = 0;
    while let Some(a) = v.pop() && { if a > 2 { z = a; } z < 10 } {
        println!("{}", a);
    }
    println!("{}", z);
}
fn issue() {
    let s = String::from("a");
    let y = Some(5);
    let mut z = 0;
    if let Some(a) = y && { if a > 2 { z = a; take(s); } z > 3 } {
        println!("big");
    } else {
        println!("{}", z);
    }
}
fn without_else() {
    let y = Some(String::from("new"));
    let mut w = String::from("old");
    drop(w);
    if let Some(a) = y && { w = a; w.len() > 5 } {
        println!("{}", w);
    }
    w = String::from("again");
    println!("{}", w);
}
fn main() { issue(); without_else(); with_else(Some(5)); no_else(Some(5)); kept(Some(5)); if_else_inner(Some(1)); while_let(vec![1]); }
