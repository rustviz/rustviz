// #570: a variable moved into a `vec![..]` or literal that nothing binds
// is moved into the `(temporary N)` column of #469 (so it stops owning
// the value), and the temporary, not the variable, drops it.
fn make() -> String {
    String::from("m")
}

struct P {
    name: String,
    n: i32,
}

fn main() {
    let s = make();
    vec![s];
    let t = make();
    let _ = P { name: t, n: 1 };
    let u = make();
    let k = (u, 1).1;
    let a = make();
    let b = make();
    let len = [a, b].len();
    println!("{} {}", k, len);
}
