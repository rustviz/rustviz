// #1402's siblings: a reference part of a value the arms of a
// conditional build from different lenders borrows all of them until
// its last use: `match` arms, `else if`, a tuple element, a `&mut`
// field, a struct inside a struct literal, an arm with a string literal.
struct One<'p> { a: &'p str }
struct M<'p> { a: &'p mut String }
struct Outer<'p> { inner: One<'p>, n: i32 }

fn match_arms() {
    let a1 = String::from("a");
    let a2 = String::from("b");
    let a3 = String::from("c");
    let n = std::env::args().count();
    let d = match n {
        0 => One { a: &a1 },
        1 => One { a: &a2 },
        _ => One { a: &a3 },
    };
    println!("{}", d.a);
}

fn else_if() {
    let b1 = String::from("a");
    let b2 = String::from("b");
    let b3 = String::from("c");
    let n = std::env::args().count();
    let e = if n == 0 {
        One { a: &b1 }
    } else if n == 1 {
        One { a: &b2 }
    } else {
        One { a: &b3 }
    };
    println!("{}", e.a);
}

fn tuple_elem() {
    let c1 = String::from("hi");
    let c2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let t = if c { (&c1, 1) } else { (&c2, 2) };
    println!("{} {}", t.0, t.1);
}

fn mut_field() {
    let mut m1 = String::from("hi");
    let mut m2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let m = if c { M { a: &mut m1 } } else { M { a: &mut m2 } };
    m.a.push('!');
    println!("{} {}", m1, m2);
}

fn nested() {
    let n1 = String::from("hi");
    let n2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let o = Outer { inner: if c { One { a: &n1 } } else { One { a: &n2 } }, n: 1 };
    println!("{} {}", o.inner.a, o.n);
}

fn literal_arm() {
    let l1 = String::from("hi");
    let c = std::env::args().count() > 1;
    let f = if c { One { a: &l1 } } else { One { a: "none" } };
    println!("{}", f.a);
}

fn main() {
    match_arms();
    else_if();
    tuple_elem();
    mut_field();
    nested();
    literal_arm();
}
