// #892: `if let` / `match` on a Box's payload pair each sub-pattern with
// the payload's part columns, as `let (s, n) = *b;` does (#890) and as
// `match t { (s, n) => .. }` does for a local tuple: only the String
// moves, out of b7.0, and n7 copies b7.1. The Box's heap allocation is
// still freed at its scope end.
struct P {
    a: String,
    b: String,
}

fn main() {
    let b7 = Box::new((String::from("a"), 1));
    if let (s7, n7) = *b7 {
        println!("{} {}", s7, n7);
    }

    let b2 = Box::new((String::from("b"), 2));
    match *b2 {
        (s, n) => println!("{} {}", s, n),
    }

    let b3 = Box::new((String::from("c"), 3));
    match *b3 {
        (s3, 0) => println!("{}", s3),
        (s3, n3) => println!("{} {}", s3, n3),
    }

    let bp = Box::new(P { a: String::from("x"), b: String::from("y") });
    match *bp {
        P { a, .. } => println!("{}", a),
    }

    let bq = Box::new(P { a: String::from("p"), b: String::from("q") });
    if let P { a: ref r, b } = *bq {
        println!("{} {}", r, b);
    }
}
