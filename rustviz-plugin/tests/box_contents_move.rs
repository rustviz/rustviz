struct P { name: String }
fn take(p: P) -> bool { p.name.len() > 0 }
fn main() {
    let b = Box::new(P { name: String::from("a") });
    take(*b);
}
