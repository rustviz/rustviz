// #1230: `std::mem::swap(&mut r1, &mut r2)` exchanges the references two
// variables hold: afterwards r1 points to b and r2 to a, so each gives
// back the borrow the other made. (`swap<T>(&mut T, &mut T)` puts both
// loans in one lifetime: a and b both stay borrowed until the later of
// the two references' last uses.) Main's arrows returned each borrow to
// the variable's old lender.
use std::mem;

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut r1 = &a;
    let mut r2 = &b;
    std::mem::swap(&mut r1, &mut r2);
    println!("{}", r1);
}

fn mutable() {
    let mut c = String::from("c");
    let mut d = String::from("d");
    let mut m1 = &mut c;
    let mut m2 = &mut d;
    mem::swap(&mut m1, &mut m2);
    m1.push('!');
    m2.push('?');
    println!("{} {}", c, d);
}
