// #1533: s holds a value after the first `if` only if c was true. A later
// conditional that assigns nothing leaves that as it was: s may (not must)
// have no value there, and a `return` drops s's value if it has one.
fn nested_return(c: bool, d: bool) {
    let mut s = String::from("a");
    let s2 = s;
    if c {
        s = s2;
    }
    if d {
        if c { return; }
    } else {
        println!("e");
    }
}

fn deferred_init(c: bool, d: bool) {
    let t;
    let t2 = String::from("a");
    if c {
        t = t2;
    }
    if d {
        if c { return; }
    } else {
        println!("e");
    }
}

fn plain_return(c: bool, d: bool) {
    let mut u = String::from("a");
    drop(u);
    if c {
        u = String::from("b");
    }
    if d { return; }
    println!("end");
}

fn main() {
    nested_return(true, false);
    deferred_init(true, false);
    plain_return(true, false);
}
