// #1026, #1027, #1029: drop-flag texts in loops, checked with rustc and a
// `Drop`-printing type (each branch outcome, loops run 0, 1 and 2 times).

// #1027: s is assigned in a loop inside another, then again in the outer
// loop's body. With c = true and i = 0, the outer loop's first time round
// reaches `s = …"c"` with s holding nothing (nothing is dropped there); a
// later time round, it holds what the time round before gave it. (Known
// wrong, not pinned: s's column is drawn as "the owner" from the inner
// loop to `s = …"c"`, #1032.)
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

struct P {
    name: String,
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

// #1026: a join inside a `for` loop's body, after a branch that moves q,
// which the body gives a new value at its top. Where the branch didn't
// move it, q's value is dropped by that assignment when the loop comes
// back round (flags = [true, false, false]: the second "b" is dropped
// where the third is assigned), or at the scope end after the last time
// round.
fn for_join(flags: Vec<bool>) {
    let mut q = String::from("a");
    for flag in flags {
        q = String::from("b");
        if flag {
            drop(q);
        }
    }
}

// The same for a field column: x.name's value is dropped when x.name is
// assigned again, or with x at its scope end. (Known wrong, not pinned:
// x's scope end says x.name is dropped "only if nothing above moved it",
// #1031.)
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

// #1029: a bare `loop` left by a `break` before the body's move, which
// moves t on odd time rounds only. k = 2: "x" from the first time round is
// moved, the second's is dropped at `t = …"z"`; k = 1: nothing is dropped
// there; k = 0: "a" is.
fn cont_then(k: usize) {
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
    t = String::from("z");
    println!("{}", t);
}
