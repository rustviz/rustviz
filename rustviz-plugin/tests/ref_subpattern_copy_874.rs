// #874: a `&` sub-pattern copies the value a reference stored in the
// matched value points to: not that value, nor a value inside it.
struct S<'a> {
    f: &'a i32,
}

fn main() {
    let n = 5;
    let orr: Option<&i32> = Some(&n);
    let Some(&y) = orr else { return };
    if let Some(&u) = orr {
        println!("{}", u);
    }
    match orr {
        Some(&c) => println!("{}", c),
        None => {}
    }
    let p = (1, 2);
    let op: Option<&(i32, i32)> = Some(&p);
    if let Some(&(a, b)) = op {
        println!("{} {}", a, b);
    }
    let s = S { f: &n };
    let S { f: &k } = s;
    let v = vec![1, 2, 3];
    if let Some(&x) = v.get(0) {
        println!("{}", x);
    }
    let Some(&w) = v.last() else { return };
    println!("{} {} {}", y, k, w);
}
