// #1138: `t = (t.1, t.0);` moves both fields out on the right-hand side
// first, so the assignment drops nothing; both are dropped at the end.
struct P { a: String, b: String }

fn swap() {
    let mut t = (String::from("a"), String::from("b"));
    t = (t.1, t.0);
    println!("{} {}", t.0, t.1);
}

fn one_side() {
    let mut u = (String::from("a"), String::from("b"));
    u = (u.1, String::from("c"));
    println!("{} {}", u.0, u.1);
}

fn struct_swap() {
    let mut p = P { a: String::from("a"), b: String::from("b") };
    p = P { a: p.b, b: p.a };
    println!("{} {}", p.a, p.b);
}

fn in_loop() {
    let mut v = (String::from("a"), String::from("b"));
    for _ in 0..2 {
        v = (v.1, v.0);
    }
    println!("{} {}", v.0, v.1);
}

fn moved_before() {
    let mut w = (String::from("a"), String::from("b"));
    let x = w.1;
    let y = w.0;
    w = (x, y);
    println!("{} {}", w.0, w.1);
}

fn main() {
    swap();
    one_side();
    struct_swap();
    in_loop();
    moved_before();
}
