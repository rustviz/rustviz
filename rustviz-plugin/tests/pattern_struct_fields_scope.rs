struct P { name: String, n: i32 }
struct E<'a> { part: &'a str, k: i32 }

fn main() {
    let o = Some(P { name: String::from("a"), n: 1 });
    if let Some(x) = o {
        println!("{}", x.n);
    }
    let m = Some(P { name: String::from("b"), n: 2 });
    match m {
        Some(y) => println!("{}", y.n),
        None => {}
    }
    let s = String::from("hello");
    let e = Some(E { part: &s[1..], k: 3 });
    match e {
        Some(z) => println!("{} {}", z.part, z.k),
        None => {}
    }
    let mut v = vec![P { name: String::from("c"), n: 4 }];
    while let Some(w) = v.pop() {
        println!("{}", w.n);
    }
}
