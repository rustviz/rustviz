// #1471: a struct (or tuple) whose reference part each branch points at
// a different lender may point into either after the conditional; rustc
// keeps both borrowed while it, or a reference taken out of it, is in
// use. Taken apart by `let W { r } = km;` (or a tuple pattern, a match
// arm, `if let`, `let r = km.r;`), r gives both back at its last use.
struct W<'a> { r: &'a mut String }
struct S<'a> { r: &'a String }

fn struct_pattern() {
    let s = String::from("s");
    let mut m1 = String::from("m");
    let mut m2 = String::from("n");
    let c = s.len() > 0;
    let km = if c { W { r: &mut m1 } } else { W { r: &mut m2 } };
    let W { r } = km;
    if c {
        r.push_str("!");
    }
    println!("{} {}", m1, m2);
}

fn shared() {
    let s = String::from("s");
    let n1 = String::from("m");
    let n2 = String::from("n");
    let c = s.len() > 0;
    let ks = if c { S { r: &n1 } } else { S { r: &n2 } };
    let S { r: rs } = ks;
    println!("{}", rs);
    println!("{} {}", n1, n2);
}

fn tuple_pattern() {
    let s = String::from("s");
    let mut t1 = String::from("m");
    let mut t2 = String::from("n");
    let c = s.len() > 0;
    let t = if c { (&mut t1, 1) } else { (&mut t2, 2) };
    let (rt, k) = t;
    rt.push_str("!");
    println!("{} {} {}", k, t1, t2);
}

fn match_arm() {
    let s = String::from("s");
    let mut a1 = String::from("m");
    let mut a2 = String::from("n");
    let n = s.len();
    let ka = match n {
        0 => W { r: &mut a1 },
        _ => W { r: &mut a2 },
    };
    match ka {
        W { r: ra } => ra.push_str("!"),
    }
    println!("{} {}", a1, a2);
}

fn if_let() {
    let s = String::from("s");
    let mut i1 = String::from("m");
    let mut i2 = String::from("n");
    let c = s.len() > 0;
    let ki = if c { W { r: &mut i1 } } else { W { r: &mut i2 } };
    if let W { r: ri } = ki {
        ri.push_str("!");
    }
    println!("{} {}", i1, i2);
}

fn field_move() {
    let s = String::from("s");
    let mut f1 = String::from("m");
    let mut f2 = String::from("n");
    let c = s.len() > 0;
    let kf = if c { W { r: &mut f1 } } else { W { r: &mut f2 } };
    let rf = kf.r;
    rf.push_str("!");
    println!("{} {}", f1, f2);
}

fn main() {
    struct_pattern();
    shared();
    tuple_pattern();
    match_arm();
    if_let();
    field_move();
}
