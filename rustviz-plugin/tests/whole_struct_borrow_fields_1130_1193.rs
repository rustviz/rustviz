// A struct or tuple borrowed as a whole lends every field with it: rustc
// rejects a use of `p.name` while `r = &mut p` is live (E0502), as it
// does a use of p. The field columns stayed solid "owner" lines (#1193).
// `let (a, b) = &mut t;` makes one loan of all of t; a points to t.0 and
// b to t.1, and t.0 can't be used until b's last use either (E0499;
// #1130).
struct P { name: String, age: u32 }
struct Q { x: String, y: String }
struct In { v: String, n: u32 }
struct O { inner: In, k: u32 }
fn show(_p: &P) {}

fn whole() {
    let mut p = P { name: String::from("Ann"), age: 30 };
    let r = &mut p;
    r.age += 1;
    r.name.push('!');
    println!("{} {}", p.name, p.age);
    let q = &p;
    show(q);
    show(q);
    println!("{}", p.age);
}
fn destructure() {
    let mut t = (String::from("a"), String::from("b"));
    let (a, b) = &mut t;
    a.push('!');
    b.push('?');
    let mut s = Q { x: String::from("x"), y: String::from("y") };
    let Q { x, y } = &mut s;
    x.push('!');
    y.push('?');
    let u = (String::from("c"), String::from("d"));
    let (c, d) = &u;
    println!("{} {}", c, d);
}
fn nested() {
    let mut o = O { inner: In { v: String::from("v"), n: 1 }, k: 2 };
    let r = &mut o.inner;
    r.n += 1;
    println!("{}", o.k);
    println!("{}", r.v);
}
fn main() {
    whole();
    destructure();
    nested();
}
