// `let` locals first assigned in a loop body (#786). The first time
// round, `s = …` initializes s; later times round it first drops the
// String an earlier iteration assigned. A `for` / `while` body may not
// run at all, so after the loop s holds a value only if it ran: Rust
// keeps a runtime drop flag, and s's scope end (t's next assignment)
// drops the old String only then. n is Copy: nothing is dropped.

fn main() {
    let mut s;
    for i in 0..3 {
        s = String::from("x");
        println!("{} {}", i, s);
    }
    println!("after for");

    let mut k = 0;
    let mut t;
    while k < 2 {
        t = String::from("y");
        println!("{}", t);
        k += 1;
    }
    t = String::from("z");
    println!("{}", t);

    let mut n;
    for i in 0..3 {
        n = i;
        println!("{}", n);
    }
    println!("after n");
}
