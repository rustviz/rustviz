// Swapping two references (#291): the value each one borrowed stays
// borrowed by the other, so neither borrow is returned at the swap.
fn main() {
    let x = String::from("x");
    let y = String::from("y");
    let mut r1 = &x;
    let mut r2 = &y;
    (r1, r2) = (r2, r1);
    println!("{} {}", r1, r2);

    let a = String::from("a");
    let b = String::from("b");
    let mut p = &a;
    let mut q = &b;
    let t = p;
    p = q;
    q = t;
    println!("{} {}", p, q);

    let mut u = String::from("u");
    let mut w = String::from("w");
    let mut m1 = &mut u;
    let mut m2 = &mut w;
    (m1, m2) = (m2, m1);
    m1.push('!');
    m2.push('?');
    println!("{} {}", u, w);
}
