struct P {
    a: i32,
}

fn match_ref() {
    let x = Some(String::from("a"));
    let y = Some(String::from("b"));
    match (x, y) {
        (Some(ref a), _) => println!("{}", a),
        _ => println!("none"),
    }
}

fn if_let_ref_mut() {
    let p = Some(String::from("p"));
    let q = Some(String::from("q"));
    if let (Some(ref mut s), Some(t)) = (p, q) {
        s.push_str(&t);
        println!("{}", s);
    }
}

fn let_ref_plain() {
    let p = P { a: 1 };
    let q = P { a: 2 };
    let (ref x, _) = (p, q);
    println!("{}", x.a);
}

fn main() {
    match_ref();
    if_let_ref_mut();
    let_ref_plain();
}
