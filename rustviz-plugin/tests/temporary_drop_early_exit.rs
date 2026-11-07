// A temporary with a `(temporary)` column (#469) that is still alive where
// a `return` or `break` leaves early is dropped there (#410), once, with
// the early-exit wording: a `while let` scrutinee's iterator, a `for`
// loop's iterator, an `if let` scrutinee, and `vec![..]` / tuple
// scrutinees of a `match`.
fn make() -> Vec<String> {
    vec![String::from("a")]
}

fn f(n: usize) -> usize {
    let mut i = 0;
    while let Some(s) = make().into_iter().next() {
        if n == 0 {
            return s.len();
        } else if n == 1 {
            break;
        } else {
            i += 1;
        }
    }
    i
}

fn g(n: usize) -> usize {
    for s in make() {
        if s.len() == n {
            return n;
        } else {
            println!("{}", s);
        }
    }
    0
}

fn h(n: usize) -> usize {
    if let Some(s) = make().pop() {
        if n == 0 {
            return 0;
        } else {
            s.len()
        }
    } else {
        2
    }
}

fn k(n: usize) -> usize {
    match vec![String::from("v")].len() {
        0 => return 0,
        _ => {}
    }
    match (String::from("p"), n) {
        (_, 0) => return 1,
        _ => 2,
    }
}

fn main() {
    f(1);
    g(1);
    h(1);
    k(1);
}
