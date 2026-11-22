// #1293: a `let mut w;` an `if` above may have assigned, given a value in
// a later conditional's branch that returns or panics. The branches that
// get past it leave w as it was, and their dashed "may have no value"
// line ran on to the join, where no dot was drawn: the leg ended in
// mid-air. The join now has its dot, saying why w may still have no value.
fn panics(a: bool, x: bool) {
    let mut w;
    if a {
        w = String::from("a");
    }
    if x {
        w = String::from("c");
        panic!("{}", w);
    }
}
fn with_else(a: bool, x: bool) -> String {
    let mut w;
    if a {
        w = String::from("a");
    }
    if x {
        w = String::from("c");
        return w;
    } else {
        println!("no");
    }
    String::new()
}
fn in_match(a: bool, k: u8) -> String {
    let mut w;
    if a {
        w = String::from("a");
    }
    match k {
        0 => {
            w = String::from("c");
            return w;
        }
        _ => println!("no"),
    }
    String::new()
}
fn main() { panics(false, false); with_else(true, false); in_match(true, 1); }
