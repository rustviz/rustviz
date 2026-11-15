// #930: a Copy `let n;` assigned only in a branch that returns (or
// panics). On the only path that reaches `n = 2;`, n has no value yet, so
// that is n's first value, as `n = 1;` is. It said "n is assigned a new
// value", as if n had an old one.

fn h(c: bool) {
    let n: i32;
    if c {
        n = 1;
        println!("{}", n);
        return;
    }
    n = 2;
    println!("{}", n);
}

fn p(c: bool) {
    let m: i32;
    if c {
        m = 1;
        panic!("{}", m);
    }
    m = 2;
    println!("{}", m);
}

fn main() {
    h(false);
    p(false);
}
