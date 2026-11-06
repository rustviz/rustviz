// A closure that uses the whole struct captures all of it: `p` and both
// of its fields are moved into `f`.
struct P { name: String, nick: String }

fn show(x: P) -> usize { x.name.len() + x.nick.len() }

fn main() {
    let p = P { name: String::from("a"), nick: String::from("b") };
    let f = move || show(p);
    println!("{}", f());
}
