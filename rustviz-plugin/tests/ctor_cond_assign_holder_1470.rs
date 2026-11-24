// #1470: a value that holds a borrow, given a constructor around a
// conditional (`Some(if c { &a } else { &b })`) by an assignment, may
// hold either branch's reference: rustc keeps a and b borrowed until its
// last use. It borrows in each branch, as `let q = Some(if ..)` and `q =
// if c { Some(&a) } else { .. }` do; also as a tuple element or a struct
// field, by assignment or by `let`.
#[derive(Debug)]
struct W<'a> { o: Option<&'a String>, n: i32 }

fn from_none() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut q = None;
    println!("{:?}", q);
    q = Some(if c { &a } else { &b });
    println!("{:?}", q);
}

fn branch_local() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut q2 = None;
    println!("{:?}", q2);
    q2 = Some(if c { let r2 = &a; r2 } else { &b });
    println!("{:?}", q2);
}

fn match_arms() {
    let a = String::from("a");
    let b = String::from("b");
    let n = a.len();
    let mut m: Option<&String> = None;
    println!("{:?}", m);
    m = Some(match n { 0 => &a, _ => &b });
    println!("{:?}", m);
}

fn mutable() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let c = a.len() > 0;
    let mut mq = None;
    mq = Some(if c { &mut a } else { &mut b });
    if let Some(x) = mq {
        x.push('!');
    }
    println!("{} {}", a, b);
}

fn from_holder() {
    let a = String::from("a");
    let b = String::from("b");
    let d = String::from("d");
    let c = a.len() > 0;
    let mut h = Some(&d);
    println!("{:?}", h);
    h = Some(if c { &a } else { &b });
    println!("{:?}", h);
}

fn in_loop() {
    let a = String::from("a");
    let b = String::from("b");
    let mut g = None;
    for i in 0..2 {
        g = Some(if i == 0 { &a } else { &b });
        println!("{:?}", g);
    }
    println!("{:?}", g);
}

fn tuple_assign() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut t = (None, 0);
    println!("{:?}", t);
    t = (Some(if c { &a } else { &b }), 1);
    println!("{:?}", t);
}

fn tuple_let() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let u = (Some(if c { &a } else { &b }), 1);
    println!("{:?}", u);
}

fn struct_assign() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut w = W { o: None, n: 0 };
    println!("{:?}", w);
    w = W { o: Some(if c { &a } else { &b }), n: 1 };
    println!("{:?}", w);
}

fn struct_let() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let x = W { o: Some(if c { &a } else { &b }), n: 1 };
    println!("{:?}", x);
}

fn field_assign() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut y = W { o: None, n: 0 };
    println!("{:?}", y);
    y.o = Some(if c { &a } else { &b });
    println!("{:?}", y);
}

fn main() {
    from_none();
    branch_local();
    match_arms();
    mutable();
    from_holder();
    in_loop();
    tuple_assign();
    tuple_let();
    struct_assign();
    struct_let();
    field_assign();
}
