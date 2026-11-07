// #483: a literal a `let` pattern is matched against is a temporary even
// when no variable is moved into it: `String::new()` and the `make()`
// results are part of it, and are dropped with it (at the end of the
// statement, or with a `ref` binding at the end of the block).
fn make() -> String {
    String::from("m")
}

fn main() {
    let (ref a, _) = (make(), make());
    let (x, _) = (make(), make());
    let [ref k, _, ..] = [make(), make(), String::new()];
    println!("{} {} {}", a, x, k);
}
