// #1036: a `let mut s;` local that may already hold a value where a
// conditional starts (a loop above may have assigned it, or a branch
// of an `if` above), given a new value in only some of its branches.
// After the conditional it holds the new value, the earlier one, or
// nothing, and only a drop flag knows: what's dropped isn't only up to
// "the branch that ran". Checked with rustc and a `Drop`-printing type
// (each branch outcome, loops run 0, 1 and 2 times).

// n = 1, x = false: the `else` ran, but "w" from the loop is dropped
// at the scope end. n = 0, x = false: nothing is.
fn n1(n: usize, x: bool) {
    let mut s;
    for _ in 0..n {
        s = String::from("w");
    }
    if x {
        s = String::from("b");
    } else {
        println!("no");
    }
}

// No `else`: n = 1, x = false drops "w" at the scope end.
fn one_armed(n: usize, x: bool) {
    let mut t;
    for _ in 0..n {
        t = String::from("w");
    }
    if x {
        t = String::from("b");
    }
}

// After an `if` above that may have assigned it: a = true, x = false
// drops "a" at the scope end.
fn two_ifs(a: bool, x: bool) {
    let mut u;
    if a {
        u = String::from("a");
    } else {
        println!("no");
    }
    if x {
        u = String::from("b");
    } else {
        println!("no");
    }
}

// A `match`: n = 1, k = 1 drops "w" at the scope end.
fn in_match(n: usize, k: u8) {
    let mut v;
    for _ in 0..n {
        v = String::from("w");
    }
    match k {
        0 => v = String::from("b"),
        _ => println!("no"),
    }
}

// Assigned after the `if`: that assignment drops "w" (n = 1, x =
// false), "b" (x = true) or nothing (n = 0, x = false).
fn assigned_after(n: usize, x: bool) {
    let mut w;
    for _ in 0..n {
        w = String::from("w");
    }
    if x {
        w = String::from("b");
    }
    w = String::from("z");
    println!("{}", w);
}

// Both arms assign it: the `else` arm's assignment drops "w" only if
// the loop assigned q (n = 1, x = false), as the `if` arm's does; it
// isn't up to a branch above.
fn both_arms(n: usize, x: bool) {
    let mut q;
    for _ in 0..n {
        q = String::from("w");
    }
    if x {
        q = String::from("b");
    } else {
        q = String::from("c");
    }
    println!("{}", q);
}

fn main() {
    n1(1, false);
    one_armed(1, false);
    two_ifs(true, false);
    in_match(1, 1);
    assigned_after(1, false);
    both_arms(1, false);
}
