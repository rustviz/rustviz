// #267: a reassignment inside an if/else or match arm drops the value
// the variable held before, on whichever path runs.
struct S {
    a: String,
    b: String,
}

fn take(_s: String) {}

fn main() {
    let mut x = String::new();
    let c = true;
    if c {
        x = String::from("a");
    } else {
        x = String::from("b");
    }
    println!("{}", x);

    let mut y = String::new();
    let n = 2;
    match n {
        0 => {
            y = String::from("zero");
        }
        _ => {
            println!("other");
        }
    }
    println!("{}", y);

    let mut s = S { a: String::new(), b: String::new() };
    if c {
        s = S { a: String::from("x"), b: String::from("y") };
    } else {
        s.a = String::from("z");
    }
    println!("{} {}", s.a, s.b);

    // Moved first, then assigned: nothing to drop at the assignment.
    let mut z = String::from("old");
    if c {
        take(z);
        z = String::from("new");
    } else {
        println!("keep");
    }
    println!("{}", z);
}
