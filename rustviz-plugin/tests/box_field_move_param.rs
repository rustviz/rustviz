struct P { name: String }
fn take(s: String) -> bool { s.len() > 0 }
fn f(b: Box<P>) {
    take(b.name);
}
fn main() {
    f(Box::new(P { name: String::from("a") }));
}
