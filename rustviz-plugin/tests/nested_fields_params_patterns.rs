// Nested struct fields (`r.tl.x`) of a fn parameter and of pattern
// bindings (`if let`, `match`, `while let`) get their own columns,
// like those of a `let`-bound struct (#239). A reference field of a
// struct parameter (`e.part: &str`) is a borrow from the caller, not
// an owner (#351).
struct P {
    x: i32,
    y: i32,
}

struct R {
    tl: P,
    w: i32,
}

struct E<'a> {
    part: &'a str,
}

fn f(r: R) -> i32 {
    let a = r.tl.x;
    a + r.w
}

fn g(e: E) -> usize {
    let p = e.part;
    p.len()
}

fn main() {
    let o = Some(R { tl: P { x: 1, y: 2 }, w: 3 });
    if let Some(m) = o {
        let b = m.tl.y;
        println!("{}", b);
    }
    let k = Some(R { tl: P { x: 4, y: 5 }, w: 6 });
    match k {
        Some(n) => {
            let c = n.tl.x;
            println!("{}", c);
        }
        None => {}
    }
    let mut v = vec![R { tl: P { x: 7, y: 8 }, w: 9 }];
    while let Some(t) = v.pop() {
        let d = t.tl.y;
        println!("{}", d);
    }
    let q = R { tl: P { x: 1, y: 2 }, w: 3 };
    let s = String::from("hi");
    println!("{} {}", f(q), g(E { part: &s }));
}
