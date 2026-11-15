// #919: a `let x;` local assigned only in a branch that returns has no
// value on any path that reaches the join (the branch that assigned it
// never gets there). The join dot said nothing (an empty tooltip); it
// now says x still has no value and why. The assignment after the join
// initializes it: nothing is dropped there.

fn h(o: Option<String>) {
    let mut acc;
    let s = String::from("s");
    if o.is_none() {
        acc = String::from("x");
        println!("{}", acc);
        return;
    }
    acc = o.unwrap();
    println!("{} {}", acc, s);
}

// The same with a let-else, whose `else` block assigns before it
// returns (#906 draws that block as a branch).
fn g(o2: Option<String>) {
    let mut acc2;
    let s2 = String::from("s");
    let Some(y) = o2 else {
        acc2 = String::from("x");
        println!("{}", acc2);
        return;
    };
    acc2 = y;
    println!("{} {}", acc2, s2);
}

// Assigned in two arms that both leave.
fn m(k: u8) {
    let acc3;
    match k {
        0 => {
            acc3 = String::from("a");
            println!("{}", acc3);
            return;
        }
        1 => {
            acc3 = String::from("b");
            panic!("{}", acc3);
        }
        _ => {}
    }
    acc3 = String::from("c");
    println!("{}", acc3);
}

fn main() {
    h(None);
    g(None);
    m(2);
}
