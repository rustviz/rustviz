// #841: the join after a branch that moved a plain (nothing to drop) value.
struct P {
    a: i32,
}

fn eat(p: P) -> i32 {
    p.a
}

fn main() {
    let p = P { a: 1 };
    let mut m = P { a: 2 };
    let keep = false;
    if keep {
        println!("{} {}", p.a, m.a);
    } else {
        eat(p);
        eat(m);
    }
}
