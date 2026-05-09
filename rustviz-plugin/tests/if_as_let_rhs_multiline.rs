// `if`/`else` as RHS of `let`, formatted across multiple lines.
// Both arms acquire `s` (BoundHere). The branch column should
// converge from the parent split into each arm's acquire dot —
// no gap between the leading and the body's first event.

fn main() {
    let n = 3;
    let s = if n > 0 {
        String::from("a")
    } else {
        String::from("b")
    };
    println!("{}", s);
}
