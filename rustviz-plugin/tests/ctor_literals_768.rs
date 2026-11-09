// #768: a constructor of literals draws no arrow (like `let x = 5;`), and
// a constructor is never named as a borrower: the variable holding the
// built value is, or the call the temporary is passed to.
struct W<'a> {
    a: &'a String,
    n: i32,
}

fn take<T>(_t: T) {}

fn main() {
    let o = Some(5);
    let r: Result<i32, ()> = Ok(1);
    let n: Option<i32> = None;
    if let Some(x) = Some(5) {
        println!("{}", x);
    }
    match Some(7) {
        Some(y) => println!("{}", y),
        None => {}
    }
    let s = String::from("hi");
    let b = Some(&s);
    let w = W { a: &s, n: 2 };
    let c = Some(w);
    let d = Some(W { a: &s, n: 3 });
    take(Some(&s));
    let m = String::from("m");
    let moved = Some(m);
    println!("{:?} {:?} {:?} {:?} {:?} {:?}", o, r, n, b, c.map(|w| w.n), moved);
    println!("{}", d.map(|w| w.n).unwrap_or(0));
}
