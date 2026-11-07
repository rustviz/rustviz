struct P { name: String }
fn take(s: String) -> bool { s.len() > 0 }
fn main() {
    let b = Box::new(P { name: String::from("a") });
    take(b.name);
}
