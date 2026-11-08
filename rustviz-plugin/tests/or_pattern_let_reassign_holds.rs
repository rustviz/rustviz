// The let or-pattern variant of the drop-flag reassignment message also
// says what the old value holds (#679).
struct P { a: Option<String>, b: Option<String> }
fn main() {
    let mut p = P { a: Some(String::from("a")), b: Some(String::from("b")) };
    let (P { a: x, .. } | P { b: x, .. }) = p;
    println!("{:?}", x);
    p.a = Some(String::from("c"));
    println!("{:?}", p.a);
}
