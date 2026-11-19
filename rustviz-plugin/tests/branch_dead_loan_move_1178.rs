// #1178: s is lent to r before the `if`; the `if` points r elsewhere and
// then moves s (allowed: the loan of s is dead on that path). After the
// join s may have been moved, so it has no column, also once r's borrow
// ends.
fn shared(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let mut r = &s;
    if c {
        r = &t;
        let m = s;
        println!("{}", m);
    }
    println!("{}", r);
}
fn mutable(c: bool) {
    let mut u = String::from("a");
    let mut w = String::from("b");
    let mut q = &mut u;
    if c {
        q = &mut w;
        drop(u);
    }
    q.push('x');
}
fn main() {
    shared(true);
    mutable(true);
}
