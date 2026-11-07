// #574: a struct update that nothing binds (`P { n: 1, ..p };`) moves
// the fields it doesn't name out of p into the literal's temporary
// (#469 / #570), which drops them at the end of the statement.
struct P {
    name: String,
    tag: String,
    n: i32,
}

fn main() {
    let p = P { name: String::from("a"), tag: String::from("t"), n: 0 };
    P { n: 1, ..p };
    let q = P { name: String::from("b"), tag: String::from("u"), n: 0 };
    let _ = P { name: String::from("c"), ..q };
    println!("{} {}", q.name, p.n);
}
