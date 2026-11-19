use std::fmt::Display;
fn sum(xs: &[i32]) -> i32 { xs.iter().sum() }
fn show(d: &dyn Display) { println!("{}", d); }
fn main() {
    let mut arr = [1, 2, 3];
    let sl = &mut arr as &mut [i32];
    sl[0] = 10;
    println!("{}", sum(&arr as &[i32]));
    let mut t = String::from("t");
    let m = &mut t as &mut String;
    m.push('x');
    let m2 = m as &mut String;
    m2.push('y');
    m.push('z');
    println!("{}", t);
    let s = String::from("s");
    let r = &s as &dyn Display;
    show(&s as &dyn Display);
    let r2 = r as &dyn Display;
    println!("{} {}", r, r2);
}
