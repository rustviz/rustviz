// #1516: `*m = f(r)` with r a shared reborrow of `*m`. The right-hand side
// runs first: r's last use, then its end, then the write through m.
struct P {
    a: String,
}

fn run(m: &mut String) -> Option<usize> {
    let r = &*m;
    *m = format!("{}!", r);
    Some(0)
}

fn local() {
    let mut s = String::from("a");
    let m = &mut s;
    let r = &*m;
    *m = format!("{}!", r);
    println!("{}", m);
}

fn copyable() {
    let mut n = 1;
    let m = &mut n;
    let r = &*m;
    *m = *r + 1;
    println!("{}", m);
}

fn in_branches(k: usize) {
    let mut s = String::from("a");
    let m = &mut s;
    let r = &*m;
    if r.len() > k {
        *m = r.to_uppercase();
    }
    let r2 = &*m;
    match r2.len() {
        0 => {}
        _ => *m = r2.clone(),
    }
    let r3 = &m[..];
    *m = String::from(r3);
    println!("{}", m);
}

fn field() {
    let mut p = P { a: String::from("a") };
    let m = &mut p;
    let r = &m.a;
    m.a = format!("{}!", r);
    println!("{}", m.a);
}

fn main() {
    let mut s = String::from("a");
    run(&mut s);
    local();
    copyable();
    in_branches(0);
    field();
}
