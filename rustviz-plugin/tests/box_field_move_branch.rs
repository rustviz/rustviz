struct P { name: String, tag: String }
fn take(s: String) -> bool { s.len() > 0 }
fn main() {
    let b = Box::new(P { name: String::from("a"), tag: String::from("t") });
    let c = true;
    if c {
        take(b.name);
    }
}
