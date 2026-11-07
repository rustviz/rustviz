// A branch that moves a value and then `break`s (#344). It never
// reaches the merge after its `if`, so inside the loop the value is
// still owned there; it is gone once the loop ends through that
// `break`. A bare `loop` whose only `break` follows the move has
// certainly moved it; a `while`, or a `loop` with another `break`,
// may not have, and the drop at scope end depends on a drop flag.
fn take(_s: String) {}

fn no_else() {
    let s = String::from("a");
    let mut i = 0;
    loop {
        i += 1;
        if i > 3 {
            take(s);
            break;
        }
    }
}

fn with_else() {
    let t = String::from("b");
    let mut i = 0;
    loop {
        i += 1;
        if i > 3 {
            take(t);
            break;
        } else {
            println!("again");
        }
    }
}

fn while_loop(v: &[i32]) {
    let u = String::from("c");
    let mut i = 0;
    while i < v.len() {
        if v[i] < 0 {
            take(u);
            break;
        }
        i += 1;
    }
}

fn two_breaks(n: i32) {
    let w = String::from("d");
    let mut i = 0;
    loop {
        i += 1;
        if i > n {
            take(w);
            break;
        }
        if i > 10 {
            break;
        }
    }
}

fn main() {
    no_else();
    with_else();
    while_loop(&[1, -1]);
    two_breaks(3);
}
