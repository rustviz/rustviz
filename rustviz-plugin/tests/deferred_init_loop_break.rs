// A `loop` that assigns a `let` local (#786). s's loop can `break`
// before the assignment, also on a later time round, after an earlier
// one assigned s: after the loop s may have no value, and its next
// assignment drops the old String only if the loop assigned one. t's
// loop leaves only by the `break` right after assigning t: after the
// loop t surely holds its String (and, immutable, is assigned once).

fn main() {
    let c = true;
    let mut s;
    loop {
        if c {
            break;
        }
        s = String::from("x");
        println!("{}", s);
    }
    s = String::from("y");
    println!("{}", s);

    let mut m = 0;
    let t;
    loop {
        m += 1;
        if m > 2 {
            t = String::from("done");
            break;
        }
    }
    println!("{}", t);
}
