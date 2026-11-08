// #469: a std macro's result (`vec![..]`, `format!(..)`) or a literal
// (`P { .. }`, `(..)`, `[..]`) that nothing binds or moves on is a
// temporary, dropped at the end of its statement, like an unbound call
// result (#457). Each gets a short `(temporary N)` column.
fn make() -> String {
    String::from("m")
}

struct P {
    name: String,
    n: i32,
}

fn main() {
    vec![String::new()];
    let _ = P { name: String::new(), n: 1 };
    let n = P { name: make(), n: 2 }.n;
    let len = format!("{}-{}", n, 1).len();
    let k = (make(), 3).1;
    let a = [make(), make()].len();
    println!("{} {} {} {}", n, len, k, a);
}
