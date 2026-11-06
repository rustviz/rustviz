// #414: assigning to a reference variable that already holds a
// reference is an assignment, not an initialization. `r = s;` copies
// the `&i32` out of s (references to `T` are Copy), `m = n;` reborrows the
// `&mut String` n holds (an assignment is a coercion site, #289), and `t = first(..)` takes the reference the
// call returns. `let u; u = &x;` is the first value u gets.
fn first<'a>(a: &'a String, _b: &'a String) -> &'a String {
    a
}

fn main() {
    let a = 1;
    let b = 2;
    let mut r = &a;
    println!("{}", r);
    r = &b;
    println!("{}", r);
    let s = &a;
    r = s;
    println!("{}", r);

    let mut x = String::from("x");
    let mut y = String::from("y");
    let mut m = &mut x;
    m.push('1');
    let n = &mut y;
    m = n;
    m.push('2');
    println!("{}", m);

    let mut t = &x;
    println!("{}", t);
    t = first(&y, &x);
    println!("{}", t);
    let u;
    u = &a;
    println!("{}", u);
}
