// #875: assigning a value that holds a reference (`q = Some(r)`) to a
// variable makes it hold the borrow until its last use.
fn main() {
    let z = String::from("z");
    let r = &z;
    let mut q: Option<&String> = None;
    q = Some(r);
    println!("{:?}", q);

    let mut a = String::from("a");
    let b = String::from("b");
    let mut t: Option<&String> = None;
    t = Some(&a);
    println!("{:?}", t);
    t = Some(&b);
    a.push('x');
    println!("{:?}", t);
}
