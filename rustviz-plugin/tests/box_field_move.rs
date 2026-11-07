struct P { name: String, tag: String, age: i32 }
fn take(s: String) -> bool { s.len() > 0 }
fn main() {
    let b = Box::new(P { name: String::from("a"), tag: String::from("t"), age: 3 });
    take(b.name);
}
