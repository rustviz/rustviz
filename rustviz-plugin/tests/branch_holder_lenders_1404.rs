// #1404: a value that holds a reference (an Option, a Vec, an array,
// an iterator) bound to an `if` / `match` whose branches borrow
// different variables may hold either branch's borrow, so rustc keeps
// every branch's lender borrowed until the value's last use. Each
// branch draws its own borrow; both lenders get it back at that use.
struct One<'p> { a: &'p String }
struct W<'p> { o: Option<&'p String> }

fn main() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let c = s1.len() > 1;
    let o = if c { Some(&s1) } else { Some(&s2) };
    println!("{:?}", o);

    let t1 = String::from("a");
    let t2 = String::from("b");
    let v = if c { vec![&t1] } else { vec![&t2] };
    println!("{:?}", v);

    let a1 = String::from("c");
    let a2 = String::from("d");
    let n = a1.len();
    let arr = match n {
        0 => [&a1],
        _ => [&a2],
    };
    println!("{:?}", arr);

    let m1 = String::from("e");
    let m: Option<&String> = if c { Some(&m1) } else { None };
    let d = match m {
        Some(x) => One { a: x },
        None => One { a: &s2 },
    };
    println!("{}", d.a);

    let v1 = vec![1, 2];
    let v2 = vec![3];
    let it = if c { v1.iter() } else { v2.iter() };
    println!("{:?}", it);

    let w1 = String::from("f");
    let w2 = String::from("g");
    let w = if n > 2 {
        let r = &w1;
        Some(r)
    } else if n > 5 {
        Some(&w2)
    } else {
        None
    };
    println!("{:?}", w);

    let mut p1 = String::from("h");
    let mut p2 = String::from("i");
    let pm = if c { Some(&mut p1) } else { Some(&mut p2) };
    if let Some(r) = pm {
        r.push('!');
    }
    println!("{} {}", p1, p2);

    let f1 = String::from("j");
    let hold = W { o: if c { Some(&f1) } else { None } };
    println!("{:?}", hold.o);
}
