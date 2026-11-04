// Every non-Copy field is moved out of `p`; only the Copy `p.age` is
// left, so nothing is dropped when `p` goes out of scope (#207).
struct P { name: String, nick: String, age: u32 }
fn main() {
    let p = P { name: String::from("a"), nick: String::from("n"), age: 3 };
    let a = p.name;
    let b = p.nick;
    println!("{} {} {}", a, b, p.age);
}
