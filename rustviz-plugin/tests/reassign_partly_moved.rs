fn after_match() {
    let mut t = (String::from("t0"), String::from("t1"));
    match t {
        (x, _) | (_, x) => println!("{}", x),
    }
    t = (String::from("t2"), String::from("t3"));
    println!("{}", t.0);
}

fn after_let() {
    let mut u = (String::from("u0"), String::from("u1"));
    let ((y, _) | (_, y)) = u;
    println!("{}", y);
    u = (String::from("u2"), String::from("u3"));
    println!("{}", u.0);
}

fn nested_part() {
    let mut v = (String::from("v0"), (String::from("v1"), String::from("v2")));
    match v {
        (_, (z, _) | (_, z)) => println!("{}", z),
    }
    v.1 = (String::from("v3"), String::from("v4"));
    println!("{}", v.1 .0);
}

fn moved_part() {
    let mut w = (String::from("w0"), String::from("w1"), String::from("w2"));
    let n = w.0;
    println!("{}", n);
    w = (String::from("w3"), String::from("w4"), String::from("w5"));
    println!("{}", w.0);
}

fn main() {
    after_match();
    after_let();
    nested_part();
    moved_part();
}
