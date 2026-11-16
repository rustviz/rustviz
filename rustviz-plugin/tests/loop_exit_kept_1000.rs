// #1000: a bare `loop` ends only at a `break`, so after it a variable
// holds what it held at the `break`s, not what the end of the body left
// (that goes back round to the start). The body is drawn once (#853), so
// where every `break` comes when the variable owns a value but the body's
// end moved it, the loop's end says it owns a value again. Checked with
// rustc (each program compiles, using the variable after the loop) and a
// `Drop`-printing type standing in for String.

// The issue's program. k = 0: the assignment drops "a" and the end of
// `main` drops "b". k = 1: the assignment drops "a", `drop(s)` the first
// "b", the second time round's assignment drops nothing, and the end of
// `main` drops the second "b".
fn main() {
    let k = 1;
    let mut s = String::from("a");
    let mut n = 0;
    loop {
        s = String::from("b");
        if n >= k {
            break;
        }
        drop(s);
        n += 1;
    }
    println!("{}", s);
}

// The move comes on a path that returns, after the `break`: r is never
// moved on the way out of the loop. c = true: "a" is dropped at the end
// of `ret`; c = false: by `drop(r)`.
fn ret(c: bool) {
    let r = String::from("a");
    loop {
        if c {
            break;
        }
        drop(r);
        return;
    }
    println!("{}", r);
}

struct P {
    name: String,
    n: i32,
}

// A struct moved whole at the end of the body, given a new value before
// the `break`: its field columns own theirs after the loop too.
fn whole(k: usize) {
    let mut p = P { name: String::from("a"), n: 1 };
    let mut n = 0;
    loop {
        p = P { name: String::from("b"), n: 2 };
        if n >= k {
            break;
        }
        drop(p);
        n += 1;
    }
    println!("{} {}", p.name, p.n);
}

// A `break` before the move and one after it: w may have been moved
// after the loop (#980), not "owns a resource".
fn mixed(k: usize) {
    let mut w = String::from("a");
    let mut n = 0;
    loop {
        w = String::from("b");
        if n >= k {
            break;
        }
        drop(w);
        if n > 5 {
            break;
        }
        n += 1;
    }
}
