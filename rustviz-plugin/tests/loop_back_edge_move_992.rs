// #992: a move near the end of a loop body (or before a `continue`) that
// nothing after gives a new value to. The loop comes back round with the
// variable moved, so at the start of the body it may have been moved: the
// first time round it holds its value, a later time round it doesn't. A
// reassignment there drops the old value only if it's still there (a drop
// flag), and after the loop the variable may have been moved. The body is
// drawn once, for every time round (#853). Checked with rustc (using the
// variable before its reassignment is E0382 "value moved here, in previous
// iteration of loop") and a `Drop`-printing type standing in for String.

// The issue's program. k = 0: the loop breaks at once and "a" is dropped
// at the end of `main`'s body. k = 1: the assignment drops "a",
// `drop(s)` drops "b", nothing is dropped at the end. k = 2: the second
// assignment drops nothing.
fn main() {
    let k = 2;
    let mut s = String::from("a");
    let mut n = 0;
    loop {
        if n >= k {
            break;
        }
        s = String::from("b");
        drop(s);
        n += 1;
    }
}

// Moved before a `continue` on odd rounds only. k = 2: round 1's
// assignment drops "a" and `drop(t)` drops its "x"; round 2's assignment
// drops nothing, and its "x" is dropped at the end. k = 1: nothing is
// dropped at the end.
fn cont(k: usize) {
    let mut t = String::from("a");
    let mut n = 0;
    loop {
        n += 1;
        if n > k {
            break;
        }
        t = String::from("x");
        if n % 2 == 1 {
            drop(t);
            continue;
        }
        println!("{}", t);
    }
}

// A branch that gives r a new value, moves it and goes round again
// (#993's join after a `continue`): after it r still holds its value, but
// the next time round starts with r moved. flags = [false, true, false]:
// round 1's `r = …y` drops "a"; round 2's `r = …x` drops "y" and
// `drop(r)` drops "x"; round 3's `r = …y` drops nothing; "y" is dropped
// at the end.
fn cont_join(flags: Vec<bool>) {
    let mut r = String::from("a");
    for flag in flags {
        if flag {
            r = String::from("x");
            drop(r);
            continue;
        }
        r = String::from("y");
    }
}

// A `while` loop ends when its condition is false: k = 0 drops "a" at the
// end, k >= 1 drops nothing there.
fn in_while(k: usize) {
    let mut w = String::from("a");
    let mut n = 0;
    while n < k {
        w = String::from("b");
        drop(w);
        n += 1;
    }
}

// The same with a `for` loop.
fn in_for(k: usize) {
    let mut u = String::from("a");
    for _ in 0..k {
        u = String::from("b");
        drop(u);
    }
}

struct P {
    name: String,
    n: i32,
}

// A field moved out at the end of the body. k = 0: p.name's "a" is
// dropped with p at the end; k >= 1: only p.n is left there.
fn field(k: usize) {
    let mut p = P { name: String::from("a"), n: 1 };
    let mut i = 0;
    loop {
        if i >= k {
            break;
        }
        p.name = String::from("b");
        drop(p.name);
        i += 1;
    }
    println!("{}", p.n);
}

// v gets a new value before the loop comes back round: it holds one at
// the start of every round, `drop(v)` is fine, and the scope end drops the
// last one.
fn kept(k: usize) {
    let mut v = String::from("a");
    for _ in 0..k {
        drop(v);
        v = String::from("b");
    }
}
