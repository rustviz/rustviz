struct P { a: String, b: String }
struct O { x: String, p: P }
fn f(o: O, c: bool, d: bool, e: bool) {
    if c { drop(o.x); }
    if d { drop(o.p.a); }
    if e {
        println!("{}", o.p.b);
        drop(o.p.b);
    } else {
        println!("{}", o.p.b);
        drop(o.p.b);
    }
}
