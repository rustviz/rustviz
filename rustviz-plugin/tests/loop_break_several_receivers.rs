// Several value `break`s (#561) reaching receivers other than a `let`:
// an assignment (the old value of `r` is dropped), a call argument, and
// a `Copy` value (`break k` copies `k`, `break 0` binds a literal).

fn take(s: String) -> usize {
    s.len()
}

fn main() {
    let c = std::env::args().count() > 1;
    let a = String::from("a");
    let b = String::from("b");
    let mut r = String::from("old");
    println!("{}", r);
    r = loop {
        if c { break a; } else { break b; }
    };
    let p = String::from("p");
    let q = String::from("q");
    let len = take(loop {
        if c { break p; } else { break q; }
    });
    let k = 5;
    let m = loop {
        if c { break k; } else { break 0; }
    };
    println!("{} {} {}", r, len, m);
}
