// #1031, #1032: what a loop that may leave a variable (or a struct's
// field) without a value does to its column, checked with rustc and a
// `Drop`-printing type (each branch outcome, loops run 0, 1, 2 and 3
// times).

struct P {
    name: String,
}

// #1032: s is assigned in a loop inside another, then again in the
// outer loop's body. With c = true and i = 0, the outer loop's first
// time round leaves the inner loop with s holding nothing (the `if c`
// moved "a", the `while` didn't run), so `s = …"c"` drops nothing; a
// later time round, s holds "c" from the time round before. s's column
// has no owner line from the inner loop's end to `s = …"c"`.
fn nested_after(c: bool, o: usize, i: usize) {
    let mut s = String::from("a");
    if c {
        drop(s);
    }
    for _ in 0..o {
        let mut n = 0;
        while n < i {
            s = String::from("b");
            n += 1;
        }
        s = String::from("c");
    }
}

// The same with a struct and its field column.
fn nested_after_struct(c: bool, o: usize, i: usize) {
    let mut p = P { name: String::from("a") };
    if c {
        drop(p);
    }
    for _ in 0..o {
        for _ in 0..i {
            p = P { name: String::from("b") };
        }
        p = P { name: String::from("c") };
    }
}

// Not assigned again in the outer loop's body: what s holds after the
// inner loop is dropped by the inner loop's next run (c = false, o = 2,
// i = 1: "b" from the first time round is dropped where the second is
// assigned), or at s's scope end.
fn nested_tail(c: bool, o: usize, i: usize) {
    let mut s = String::from("a");
    if c {
        drop(s);
    }
    for _ in 0..o {
        let mut n = 0;
        while n < i {
            s = String::from("b");
            n += 1;
        }
    }
}

// The inner loop a bare `loop` left by a `break` before the assignment.
fn nested_break(c: bool, o: usize, i: usize) {
    let mut s = String::from("a");
    if c {
        drop(s);
    }
    for _ in 0..o {
        let mut n = 0;
        loop {
            if n >= i {
                break;
            }
            s = String::from("b");
            n += 1;
        }
        s = String::from("c");
    }
}

// #1031: a loop that moves x.name on its first time round only. With
// k = 2, `drop(x.name)` moved the first "b", and the second is dropped
// at x's scope end; with k = 1, nothing is dropped there.
fn field_join(k: usize) {
    let mut x = P { name: String::from("a") };
    let mut n = 0;
    while n < k {
        x.name = String::from("b");
        if n == 0 {
            drop(x.name);
        }
        n += 1;
    }
}

struct Q {
    a: String,
    b: String,
}

// The same with two fields.
fn fields_join(k: usize) {
    let mut y = Q { a: String::from("a"), b: String::from("b") };
    let mut n = 0;
    while n < k {
        y.a = String::from("c");
        y.b = String::from("d");
        if n == 0 {
            drop(y.a);
            drop(y.b);
        }
        n += 1;
    }
}
