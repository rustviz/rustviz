// #1514: after a loop that may not have assigned s, an `if`/`else` (or a
// `match`) whose arm assigns s in a conditional of its own: s may still
// have no value after the join.
fn one_inner_arm(c: bool) {
    let mut s = String::new();
    drop(s);
    for _ in 0..2 {
        s = String::from("x");
    }
    if c {
        if c { s = String::from("a"); }
    } else {
    }
}

fn both_inner_arms(n: i32, c: bool) {
    let mut t = String::new();
    drop(t);
    for _ in 0..2 {
        t = String::from("x");
    }
    match n {
        0 => {
            if c { t = String::from("a"); } else { t = String::from("b"); }
            println!("after");
        }
        _ => {}
    }
}

fn main() {
    one_inner_arm(true);
    both_inner_arms(0, true);
}
