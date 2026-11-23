// `(z, make_q().name)` as an `if let` / `match` scrutinee: building the
// tuple moves `name` out of make_q()'s temporary first; the pattern takes
// it (if at all) from the tuple. n / k come from the call, as for a
// call's whole result (`(z, mk())`), and the temporary's drop leaves out
// name (#1350).
struct Q { name: String, tag: String }
fn make_q() -> Q { Q { name: String::from("n"), tag: String::from("t") } }
fn main() {
    let z = Some(String::from("z"));
    if let (Some(d), n) = (z, make_q().name) {
        println!("{} {}", d, n);
    }
    let w = Some(String::from("w"));
    match (w, make_q().name) {
        (Some(e), k) => println!("{} {}", e, k),
        _ => println!("none"),
    }
    let (a, m) = (1, make_q().name);
    println!("{} {}", a, m);
}
