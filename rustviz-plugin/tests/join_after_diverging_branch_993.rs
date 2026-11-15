// #993: a reassignment after a conditional whose branch that moved the
// variable left early (`return`, `break`, a panic). Every path that
// reaches the assignment still holds the old value, so it is always
// dropped there: no drop flag decides it. Checked with rustc and a
// `Drop`-printing type standing in for String.

// The issue's program. c = true: `drop(y)` drops "h" and the function
// returns. c = false: the assignment drops "h"; "i" is dropped at the end.
fn main() {
    let c = false;
    let mut y = String::from("h");
    if c {
        drop(y);
        return;
    }
    y = String::from("i");
    println!("{}", y);
}

// The branch that moved z breaks out of the loop: the assignment drops
// "h" the first time round and the previous "i" on later ones. After the
// loop z may have been moved (by the `break`).
fn breaks(v: Vec<i32>) {
    let mut z = String::from("h");
    for x in v {
        if x < 0 {
            drop(z);
            break;
        }
        z = String::from("i");
    }
}

// A `match` arm that panics.
fn panics(n: i32) {
    let mut m = String::from("h");
    match n {
        0 => {
            drop(m);
            panic!("zero");
        }
        _ => {}
    }
    m = String::from("i");
    println!("{}", m);
}
