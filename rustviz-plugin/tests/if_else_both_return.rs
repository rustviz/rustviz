// Every branch of the `if` / `else` returns, so the code after it (the
// merge and the closing brace) is unreachable. `r` is dropped at each
// `return`; `q` is moved to the caller at both (#515).
fn early(c: bool) -> String {
    let r = String::from("first");
    let q = String::from("second");
    if c {
        return q;
    } else {
        return q + "!";
    }
}

// The same for a `match` whose arms all return.
fn by_match(n: i32) -> String {
    let a = String::from("first");
    let b = String::from("second");
    match n {
        0 => return b,
        _ => return b + "!",
    }
}

fn main() {
    println!("{}", early(true));
    println!("{}", by_match(1));
}
