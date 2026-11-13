// #841: a moved non-Copy value with nothing to drop. `p` is not `mut`, so
// it is never assigned again; `m` is, so its tooltip keeps the clause.
struct P {
    a: i32,
}

fn main() {
    let p = P { a: 1 };
    let q = p;
    let mut m = P { a: 2 };
    let n = m;
    println!("{} {}", q.a, n.a);
}
