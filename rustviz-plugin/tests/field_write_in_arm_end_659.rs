// #659: a field write in an `if` arm whose old lender stays borrowed
// until the struct's last use, after the `if` (the #531 rule).
struct I<'a> { r: &'a String }
struct O<'a> { inner: I<'a>, n: i32 }

fn mk(r: &String) -> I<'_> { I { r } }

fn main() {
    let c = true;
    // A reference field assigned a new reference.
    let s = String::from("s");
    let t = String::from("t");
    let x = String::from("x");
    let mut o = O { inner: I { r: &s }, n: 1 };
    if c {
        o.inner.r = &t;
        drop(x);
    }
    println!("{}", o.n);
    // The field's struct overwritten by a literal, a call, a variable.
    let s2 = String::from("s2");
    let t2 = String::from("t2");
    let x2 = String::from("x2");
    let mut o2 = O { inner: I { r: &s2 }, n: 2 };
    if c {
        o2.inner = I { r: &t2 };
        drop(x2);
    }
    println!("{}", o2.n);
    let s3 = String::from("s3");
    let t3 = String::from("t3");
    let x3 = String::from("x3");
    let mut o3 = O { inner: I { r: &s3 }, n: 3 };
    if c {
        o3.inner = mk(&t3);
        drop(x3);
    }
    println!("{}", o3.n);
    let s4 = String::from("s4");
    let t4 = String::from("t4");
    let x4 = String::from("x4");
    let other = I { r: &t4 };
    let mut o4 = O { inner: I { r: &s4 }, n: 4 };
    if c {
        o4.inner = other;
        drop(x4);
    }
    println!("{}", o4.n);
}
