fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let pair = (&a, &b);
    let (x, y) = pair;
    println!("{}", x);
    // x's last use was above: a is free, b is still borrowed by y.
    a.push('!');
    println!("{}", y);
    b.push('!');
    let mut c = String::from("c");
    let mut d = String::from("d");
    let pair2 = (&c, &d);
    match pair2 {
        (p, q) => {
            println!("{}", p);
            c.push('!');
            println!("{}", q);
            d.push('!');
        }
    }
    let e = String::from("e");
    let f = String::from("f");
    let pair3 = (&e, &f);
    if let (u, v) = pair3 {
        println!("{}", u);
        println!("{}", v);
    }
}
