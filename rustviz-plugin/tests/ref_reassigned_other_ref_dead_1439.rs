// A reference given a new value (`r1 = &t1;`) while another reference to
// the same lender exists whose loan is already over on the paths to the
// write (#1439). The other reference used to count as still borrowing, so
// r's old loan gave nothing back: in an `if`, r's other arm had no end at
// all and r's legs joined with no dot; in straight-line code the other
// reference's earlier end said the lender was "no longer immutably
// borrowed" while r still borrowed it. Each function is one context.

// The issue's program: x1 is read only in the other arm.
fn other_ref_in_other_arm(c: bool) {
    let s1 = String::from("x");
    let t1 = String::from("y");
    let mut r1 = &s1;
    let x1 = &s1;
    if c {
        r1 = &t1;
    } else {
        println!("{} {}", r1, x1);
    }
}

// x2 is never used: its loan ends where it's made.
fn other_ref_unused(c: bool) {
    let s2 = String::from("x");
    let t2 = String::from("y");
    let mut r2 = &s2;
    let x2 = &s2;
    if c {
        r2 = &t2;
    } else {
        println!("{}", r2);
    }
}

// Straight-line: x3's last use comes before r3's.
fn straight_line() {
    let s3 = String::from("x");
    let t3 = String::from("y");
    let mut r3 = &s3;
    let x3 = &s3;
    x3.len();
    r3.len();
    r3 = &t3;
    r3.len();
}

// A `match` arm instead of an `if`.
fn match_arm(n: i32) {
    let s4 = String::from("x");
    let t4 = String::from("y");
    let mut r4 = &s4;
    let x4 = &s4;
    match n {
        0 => r4 = &t4,
        _ => println!("{} {}", r4, x4),
    }
}

// x5 is read after the `if`: it keeps s5 borrowed, and r5's old loan,
// ended in each arm, says so.
fn other_ref_used_after(c: bool) {
    let s5 = String::from("x");
    let t5 = String::from("y");
    let mut r5 = &s5;
    let x5 = &s5;
    if c {
        r5 = &t5;
    } else {
        println!("{}", r5);
    }
    println!("{}", x5);
}

// The write goes through `*w`.
fn through_deref() {
    let s6 = String::from("x");
    let t6 = String::from("y");
    let mut r6 = &s6;
    let x6 = &s6;
    x6.len();
    r6.len();
    let w6 = &mut r6;
    *w6 = &t6;
    println!("{}", r6);
}

// A destructuring assignment of two references.
fn destructuring() {
    let s7 = String::from("x");
    let t7 = String::from("y");
    let mut r7 = &s7;
    let mut q7 = &s7;
    let x7 = &s7;
    x7.len();
    r7.len();
    q7.len();
    (r7, q7) = (&t7, &t7);
    println!("{} {}", r7, q7);
}

// In a loop body.
fn in_loop() {
    let s8 = String::from("x");
    let t8 = String::from("y");
    let mut r8 = &s8;
    let x8 = &s8;
    x8.len();
    for _ in 0..2 {
        r8.len();
        r8 = &t8;
    }
}

// x9 is read in the same arm, before the write.
fn other_ref_read_before_write(c: bool) {
    let s9 = String::from("x");
    let t9 = String::from("y");
    let mut r9 = &s9;
    let x9 = &s9;
    if c {
        println!("{}", x9);
        r9 = &t9;
    } else {
        println!("{}", r9);
    }
}

fn main() {
    other_ref_in_other_arm(false);
    other_ref_unused(false);
    straight_line();
    match_arm(1);
    other_ref_used_after(false);
    through_deref();
    destructuring();
    in_loop();
    other_ref_read_before_write(false);
}
