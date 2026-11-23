// #1284: moving a struct with nothing to drop (`P { a: i32 }`) into a
// literal moves its fields with it: `p` isn't Copy, so `p.a` can't be used
// after it either. main left p.a's column owned to its scope end when the
// literal has no column of its own (`let (x, _) = (s, p);`, #1163) and when
// a `match` / `if let` scrutinee's tuple took it (Copy fields were left out).
struct P { a: i32 }
struct Q { b: P }
fn let_literal(s: String, p: P) {
    let (x, _) = (s, p);
    println!("{}", x);
}
fn let_ref_literal() {
    let p = P { a: 1 };
    let q = P { a: 2 };
    let (ref x, _) = (p, q);
    println!("{}", x.a);
}
fn assign_literal(s: String) {
    let p = P { a: 3 };
    let x;
    (x, _) = (s, p);
    println!("{}", x);
}
fn match_tuple(s: String) {
    let p = P { a: 4 };
    match (s, p) {
        (x, _) => println!("{}", x),
    }
}
fn if_let_tuple(s: String) {
    let p = P { a: 5 };
    if let (x, _) = (s, p) {
        println!("{}", x);
    }
}
fn nested_struct(s: String) {
    let q = Q { b: P { a: 6 } };
    let (x, _) = (s, q);
    println!("{}", x);
}
fn main() {
    let_literal(String::new(), P { a: 0 });
    let_ref_literal();
    assign_literal(String::new());
    match_tuple(String::new());
    if_let_tuple(String::new());
    nested_struct(String::new());
}
