// #886: a destructuring `let` whose right side is `*r` binds copies of
// values inside `*r`, as `match *r` / `if let` do; a whole `let x = *r;`
// copies `*r` itself.
struct P {
    x: i32,
    y: i32,
}

struct W(i32, i32);

fn main() {
    let t = (1, 2);
    let r = &t;
    let (c, d) = *r;
    match *r {
        (a, b) => println!("{} {}", a, b),
    }
    let whole = *r;
    let p = P { x: 3, y: 4 };
    let rp = &p;
    let P { x, y } = *rp;
    let w = W(5, 6);
    let rw = &w;
    let W(m, n) = *rw;
    let bx = Box::new((7, 8));
    let (g, h) = *bx;
    println!("{} {} {} {} {} {} {} {:?} {} {}", c, d, x, y, m, n, whole.0, whole, g, h);
}
