// #1368: a binding by reference into what a call's returned reference
// points to, when the signature ties that reference to two arguments:
// it may point into either, so both stay borrowed while it is in use
// (`o1.take()` in the body is E0502), as for `let r = pick2(&o1, &o2);`.
struct P {
    name: String,
    n: i32,
}

fn pick2<'a>(a: &'a Option<String>, b: &'a Option<String>) -> &'a Option<String> {
    if a.is_some() { a } else { b }
}

fn pick_p<'a>(a: &'a P, b: &'a P) -> &'a P {
    if a.n > b.n { a } else { b }
}

fn pick_mut<'a>(a: &'a mut Option<String>, b: &'a mut Option<String>) -> &'a mut Option<String> {
    if a.is_some() { a } else { b }
}

fn if_let() {
    let a1 = Some(String::from("x"));
    let b1 = Some(String::from("y"));
    if let Some(s1) = pick2(&a1, &b1) {
        println!("{}", s1);
    }
}

fn match_arm() {
    let a2 = Some(String::from("x"));
    let b2 = Some(String::from("y"));
    match pick2(&a2, &b2) {
        Some(s2) => println!("{}", s2),
        None => {}
    }
}

// A guard reads s3 through a reference to it; s3's borrow still lasts
// to its use in the arm.
fn match_guard() {
    let a3 = Some(String::from("x"));
    let b3 = Some(String::from("y"));
    match pick2(&a3, &b3) {
        Some(s3) if s3.len() > 1 => {
            println!("{}", s3);
        }
        _ => {}
    }
}

fn struct_fields() {
    let a4 = P { name: String::from("a"), n: 1 };
    let b4 = P { name: String::from("b"), n: 2 };
    if let P { name: s4, n: 2 } = pick_p(&a4, &b4) {
        println!("{}", s4);
    }
}

fn while_let() {
    let a5 = Some(String::from("x"));
    let b5 = Some(String::from("y"));
    let mut k = 0;
    while let Some(s5) = pick2(&a5, &b5) {
        println!("{}", s5);
        k += 1;
        if k > 2 {
            break;
        }
    }
}

fn mutable() {
    let mut a6 = Some(String::from("x"));
    let mut b6 = Some(String::from("y"));
    if let Some(s6) = pick_mut(&mut a6, &mut b6) {
        s6.push('!');
    }
    println!("{:?} {:?}", a6, b6);
}

fn main() {
    if_let();
    match_arm();
    match_guard();
    struct_fields();
    while_let();
    mutable();
}
