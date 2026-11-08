// `r = loop { … }` (#586): the loop runs first, and `r`'s old String is
// dropped when the loop's value is assigned to `r`, after the loop ends,
// on the same row as the new value's arrival, not on the `r = loop {`
// line before the body runs.

fn main() {
    let a = String::from("a");
    let mut i = 0;
    let mut r = String::from("old");
    println!("{}", r);
    r = loop {
        i += 1;
        if i == 3 { break a; }
    };
    println!("{}", r);
}

fn used_in_loop() {
    let a = String::from("a");
    let mut i = 0;
    let mut s = String::from("old");
    s = loop {
        i += 1;
        println!("{}", s);
        if i < 3 { continue; }
        break a;
    };
    println!("{}", s);
}

fn several() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut t = String::from("old");
    println!("{}", t);
    t = loop {
        if c { break a; } else { break b; }
    };
    println!("{}", t);
}
