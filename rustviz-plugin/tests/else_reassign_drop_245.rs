// #245: a reassignment in the `else` branch drops the value the variable
// holds on that path. The if branch moved it, but that move isn't on the
// `else`'s path.
fn take(_s: String) {}

// The same with a by-value parameter, which holds the caller's value.
fn param(c: bool, mut p: String) {
    if c {
        take(p);
    } else {
        p = String::from("c");
        println!("{}", p);
    }
}

fn if_let(o: Option<i32>, mut t: String) {
    if let Some(n) = o {
        println!("{}", n);
        take(t);
    } else {
        t = String::from("c");
        println!("{}", t);
    }
}

fn else_if(k: i32, mut u: String) {
    if k == 0 {
        take(u);
    } else if k == 1 {
        u = String::from("c");
        println!("{}", u);
    } else {
        u = String::from("d");
        println!("{}", u);
    }
}

// The other way round: the if branch's reassignment drops, the else
// branch moves.
fn reverse(c: bool, mut w: String) {
    if c {
        w = String::from("b");
        println!("{}", w);
    } else {
        take(w);
    }
}

// Without a branch: reassigning a by-value parameter drops its value.
fn reassign_param(mut q: String) {
    q = String::from("b");
    println!("{}", q);
}

// A Copy value has nothing to drop.
fn copy(c: bool, mut n: i32) {
    if c {
        let m = n;
        println!("{}", m);
    } else {
        n = 6;
        println!("{}", n);
    }
}

fn main() {
    let c = true;
    let mut s = String::from("a");
    if c {
        take(s);
    } else {
        s = String::from("c");
        println!("{}", s);
    }
    param(true, String::from("a"));
    if_let(Some(1), String::from("a"));
    else_if(1, String::from("a"));
    reverse(false, String::from("a"));
    reassign_param(String::from("a"));
    copy(true, 5);
}
