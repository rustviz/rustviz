// `if`/`else` as the RHS of a `let`. The plugin doesn't track
// borrows *inside* a conditional body (see Limitations) but using
// the conditional as an expression that produces a value into a
// `let` is supported.

fn main() {
    let n = 3;
    let s = if n > 0 { String::from("a") } else { String::from("b") };
    println!("{}", s);
}
