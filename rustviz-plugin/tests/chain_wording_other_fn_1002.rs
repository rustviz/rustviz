// #1002: whether a drop-flag text says "nothing above moved it" or "no
// branch above moved it" is decided from the variable's own events. `st`'s
// loop moves its p on the way to a `break` outside any branch, so `st`'s
// texts say "nothing above"; `st2`'s p, a different variable with the
// same name, is moved only in a branch, so its texts say "no branch
// above", as they do when `st2` is alone in the file.

struct P {
    name: String,
    n: i32,
}

fn st(c: bool) {
    let mut p = P { name: String::from("a"), n: 1 };
    loop {
        drop(p);
        if c {
            break;
        }
        p = P { name: String::from("b"), n: 2 };
    }
}

fn st2(c: bool) {
    let mut p = P { name: String::from("a"), n: 1 };
    if c {
        drop(p);
    }
    p = P { name: String::from("b"), n: 2 };
    println!("{}", p.n);
}

fn main() {
    st(true);
    st2(true);
}
