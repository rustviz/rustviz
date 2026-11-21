// A value holding a borrow, moved by a pattern or into another variable
// inside a conditional, carries the borrow with it: the moved-from
// column draws no end of it afterwards (#1198).
// A reference field given another struct's reference with its struct
// (`o.inner = other;`) keeps its line to its last use (#1199).
struct Part<'a> { part: &'a str }
struct I<'a> { r: &'a String }
struct O<'a> { inner: I<'a>, n: i32 }

fn if_let_holder() {
    let s = String::from("hello");
    let o = Some(Part { part: &s });
    if let Some(x) = o {
        let q = x.part;
        println!("{}", q);
    }
}

fn match_holder() {
    let s2 = String::from("world");
    let p = Some(Part { part: &s2 });
    match p {
        Some(y) => println!("{}", y.part),
        None => {}
    }
}

fn vec_holder_in_arm(c: bool) {
    let s3 = String::from("v");
    let u = vec![&s3];
    if c {
        let x = u;
        println!("{:?}", x);
    }
}

fn field_from_other() {
    let mut s = String::from("s");
    let t = String::from("t");
    let mut o = O { inner: I { r: &s }, n: 1 };
    let other = I { r: &t };
    o.inner = other;
    println!("{} {}", o.inner.r, o.n);
    s.push('!');
}

fn main() {
    if_let_holder();
    match_holder();
    vec_holder_in_arm(true);
    field_from_other();
}
