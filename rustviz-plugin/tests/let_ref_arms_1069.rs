fn main() {
    let s = String::from("a");
    let t = String::from("b");
    let c = s.len() > 0;
    let r;
    if c {
        r = &t;
    } else {
        r = &s;
    }
    println!("{}", r);
    mutable(c);
    three(s.len());
    inner(c);
}

fn mutable(c: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let m;
    if c {
        m = &mut b;
    } else {
        m = &mut a;
    }
    m.push('x');
    println!("{} {}", a, b);
}

fn three(n: usize) {
    let x = String::from("x");
    let y = String::from("y");
    let z = String::from("z");
    let r;
    match n {
        0 => r = &x,
        1 => r = &y,
        _ => r = &z,
    }
    println!("{}", r);
}

fn inner(c: bool) {
    let p = String::from("p");
    let q = String::from("q");
    let r;
    if c {
        r = &p;
        println!("{}", r);
    } else {
        r = &q;
        println!("{}", r);
    }
}
