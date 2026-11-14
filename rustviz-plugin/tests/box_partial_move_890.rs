// #890: destructuring a Box's payload moves only the parts the pattern
// binds by value, out of their own columns (`b2.0`, `bp.a`), as
// `let (s, n) = t;` does for a local tuple. The Box's heap allocation is
// still freed at its scope end, with whatever the pattern left in it.
struct P {
    a: String,
    b: String,
}

struct Q {
    s: String,
    k: i32,
}

struct O {
    pair: (String, i32),
    k: i32,
}

fn main() {
    let b2 = Box::new((String::from("a"), 1));
    let (s, n) = *b2;
    println!("{} {}", s, n);

    let bp = Box::new(P { a: String::from("x"), b: String::from("y") });
    let P { a, .. } = *bp;
    println!("{}", a);

    let bq = Box::new(Q { s: String::from("q"), k: 3 });
    let Q { s: s2, k } = *bq;
    println!("{} {}", s2, k);

    let b4 = Box::new((String::from("c"), String::from("d")));
    let (x, y) = *b4;
    println!("{} {}", x, y);

    let bo = Box::new(O { pair: (String::from("e"), 4), k: 5 });
    let (s3, m) = (*bo).pair;
    println!("{} {} {}", s3, m, bo.k);
}
