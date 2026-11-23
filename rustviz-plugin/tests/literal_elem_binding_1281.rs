// #1281: a name a pattern binds from a literal in the scrutinee's tuple
// (`n` in `(s, n) => …` on `match (&t, 1)`) gets the value there, as the
// `let` form draws it: "n is bound to a copyable value". main drew no
// event giving it one (State::Invalid; its first dot was a use).
struct P { a: i32 }
fn borrow_and_literal() {
    let t = String::from("t");
    match (&t, 1) { (s, n) => println!("{} {}", s, n) }
}
fn move_and_literal() {
    let t = String::from("t");
    match (t, 2) { (s, m) => println!("{} {}", s, m) }
}
fn if_let_literal() {
    let t = String::from("t");
    if let (s, k) = (&t, 3) { println!("{} {}", s, k); }
}
fn nested_literal() {
    let t = String::from("t");
    match (&t, (4, 5)) { (s, (a, b)) => println!("{} {} {}", s, a, b) }
}
fn struct_literal() {
    let t = String::from("t");
    match (&t, P { a: 6 }) { (s, p) => println!("{} {}", s, p.a) }
}
fn let_else_literal() {
    let t = String::from("t");
    let (s, f) = (&t, 1.5) else { return };
    println!("{} {}", s, f);
}
fn main() {
    borrow_and_literal();
    move_and_literal();
    if_let_literal();
    nested_literal();
    struct_literal();
    let_else_literal();
}
