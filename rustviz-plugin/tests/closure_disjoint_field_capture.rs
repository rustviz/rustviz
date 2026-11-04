// Since Rust 2021 a closure captures only the places it uses (disjoint
// captures, RFC 2229): `move || p.name.len()` moves `p.name` into `f`
// and leaves `p.nick` with `p`, so `p.nick` can still be used and is
// dropped at the end of main. `|| q.nick.len()` borrows only `q.nick`.
struct P { name: String, nick: String }

fn main() {
    let p = P { name: String::from("a"), nick: String::from("b") };
    let f = move || p.name.len();
    println!("{} {}", f(), p.nick);
    let q = P { name: String::from("c"), nick: String::from("d") };
    let g = || q.nick.len();
    println!("{}", g());
    let r = q.name;
    println!("{}", r);
}
