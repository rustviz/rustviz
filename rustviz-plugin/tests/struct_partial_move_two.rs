// Partial moves stay per-field: `p.name` moves out, `p.age` is copied,
// and `p.nick` is still owned (and dropped) by `p`.
struct P { name: String, nick: String, age: u32 }
fn main() {
    let p = P { name: String::from("a"), nick: String::from("n"), age: 3 };
    let n = p.name;
    let a = p.age;
    println!("{} {} {}", n, a, p.nick);
}
