// #469: a field moved out of a temporary (`make_q().a`). The rest of
// the temporary is dropped at the end of the statement.
struct P {
    name: String,
    n: i32,
}

struct Q {
    a: String,
    b: String,
}

fn make_p() -> P {
    P { name: String::from("p"), n: 1 }
}

fn make_q() -> Q {
    Q { a: String::from("a"), b: String::from("b") }
}

fn pair() -> (String, String) {
    (String::from("x"), String::from("y"))
}

fn take(s: String) {
    println!("{}", s);
}

fn main() {
    let t = make_q().a;
    take(make_q().b);
    let first = pair().0;
    let s = make_p().name;
    let n = make_q().a.len();
    println!("{} {} {} {}", t, first, s, n);
}
