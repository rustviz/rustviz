// #1001, the mirror of #992: a variable moved before a loop and given a
// value in its body. The first time round the assignment drops nothing
// (the variable was moved), but a later time round it drops what the time
// round before gave it, if the variable still holds it (a drop flag). And
// the loop can end before giving it a value (a `break` first, or a
// `while` / `for` that stops), so after the loop a drop flag decides
// whether its scope end drops anything. Checked with a `Drop`-printing
// type standing in for String, for k = 0, 1 and 2.

// The issue's program. k = 0: "a" is dropped by `drop(s)`, nothing at
// the end. k = 2: the second assignment drops the first "b", the end of
// `main` the second.
fn main() {
    let k = 2;
    let mut s = String::from("a");
    drop(s);
    let mut n = 0;
    loop {
        if n >= k {
            break;
        }
        s = String::from("b");
        n += 1;
    }
}

// The same with a `while` loop: k = 0 drops nothing at the end.
fn wh(k: usize) {
    let mut t = String::from("a");
    drop(t);
    let mut n = 0;
    while n < k {
        t = String::from("b");
        n += 1;
    }
}

// And a `for` loop.
fn fr(k: usize) {
    let mut u = String::from("a");
    drop(u);
    for _ in 0..k {
        u = String::from("b");
    }
}

// The `break` comes after the assignment: w always holds a value after
// the loop (and its scope end always drops it), but the assignment still
// drops what the time round before gave it.
fn brk_after(k: usize) {
    let mut w = String::from("a");
    drop(w);
    let mut n = 0;
    loop {
        w = String::from("b");
        if n >= k {
            break;
        }
        n += 1;
    }
    println!("{}", w);
}

// Assigned again after the loop: that drops the loop's value only if the
// loop gave it one. k = 0: nothing; k = 1: "b".
fn after(k: usize) {
    let mut r = String::from("a");
    drop(r);
    let mut n = 0;
    loop {
        if n >= k {
            break;
        }
        r = String::from("b");
        n += 1;
    }
    r = String::from("c");
    println!("{}", r);
}

// The assignment is in an inner loop, which may end before it runs, and
// so may the outer one. k = 0: nothing is dropped at the end.
fn nested(k: usize) {
    let mut q = String::from("a");
    drop(q);
    let mut n = 0;
    loop {
        loop {
            if n >= k {
                break;
            }
            q = String::from("b");
            n += 1;
        }
        if n >= k {
            break;
        }
    }
}

struct P {
    name: String,
    n: i32,
}

// A struct moved whole before the loop: its field columns too.
fn whole(k: usize) {
    let mut p = P { name: String::from("a"), n: 1 };
    drop(p);
    let mut n = 0;
    loop {
        if n >= k {
            break;
        }
        p = P { name: String::from("b"), n: 2 };
        n += 1;
    }
}
