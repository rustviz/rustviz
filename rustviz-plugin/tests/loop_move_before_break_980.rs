// #980: a move in a loop body before a `break` that isn't in the branch
// that breaks. The loop ends only through its `break`s (or a `while`'s
// condition), so what s holds after the loop is what it holds at each of
// them, not at the end of the body. Checked with rustc (using s after the
// loop is E0382 in `main`; maybe-moved in `some_breaks` and `in_while`)
// and a `Drop`-printing type standing in for String.

// Every way out of the loop comes after `drop(s)`: the new value is for
// the next iteration only, so s is moved after the loop and its scope end
// drops nothing ("a" and "b" are both dropped by `drop(s)`).
fn main() {
    let mut s = String::from("a");
    let mut n = 0;
    loop {
        drop(s);
        if n > 0 {
            break;
        }
        s = String::from("b");
        n += 1;
    }
}

// The `let … else` leaves before the move, the `if` after it: t may have
// been moved. With [1, -1], "b" and "c" are dropped by `drop(t)` and
// nothing at the end; with [1, 2], "c" is dropped at the end of the
// function.
fn some_breaks(v: Vec<i32>) {
    let mut t = String::from("b");
    let mut it = v.into_iter();
    loop {
        let Some(x) = it.next() else { break };
        drop(t);
        if x <= 0 {
            break;
        }
        t = String::from("c");
    }
}

// u gets a new value before the `break`: it owns it after the loop, and its
// scope end drops it ("e" is dropped at the end of the function).
fn restored(n: i32) {
    let mut u = String::from("d");
    let mut i = 0;
    loop {
        drop(u);
        u = String::from("e");
        if i >= n {
            break;
        }
        i += 1;
    }
}

// A `while` loop also ends when its condition is false, after the body gave
// w a new value; its `break` comes after the move. w may have been moved:
// with n = 5 the loop breaks and nothing is dropped at the end; with n = 1
// the condition ends it and "g" is dropped there.
fn in_while(n: i32) {
    let mut w = String::from("f");
    let mut i = 0;
    while i < n {
        drop(w);
        if i == 2 {
            break;
        }
        w = String::from("g");
        i += 1;
    }
}

// Moved in the branch that breaks, given a new value after it: the other
// `break` leaves with that value, so y may have been moved, and assigning
// it after the loop drops the old value only if it wasn't. With [1, -1],
// "h" and "i" are dropped by `drop(y)` and the assignment drops nothing;
// with [1], the assignment drops "i".
fn branch_then_reassign(v: Vec<i32>) {
    let mut y = String::from("h");
    let mut it = v.into_iter();
    loop {
        let Some(x) = it.next() else { break };
        if x < 0 {
            drop(y);
            break;
        }
        y = String::from("i");
    }
    y = String::from("j");
    println!("{}", y);
}
