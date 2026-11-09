// A `let` assigned on only some paths (#244). After each conditional
// the variable holds a value only if the branch that ran assigned it.
// Rust can't read it there (E0381) and keeps a runtime drop flag for
// it: s's next assignment drops the old String only if there is one,
// and t's scope end drops its String only if the `0` arm ran (the `_`
// arm panics, so it never gets past the match). n is Copy: nothing is
// dropped either way.

fn main() {
    let c = true;
    let mut s;
    if c {
        s = String::from("maybe");
        println!("{}", s);
    }
    s = String::from("surely");
    println!("{}", s);

    let mut n;
    if c {
        n = 5;
        println!("{}", n);
    }
    n = 6;
    println!("{}", n);

    let k = 1;
    let t;
    match k {
        0 => t = String::from("zero"),
        1 => println!("one"),
        _ => unreachable!(),
    }
    println!("{}", k);
}
