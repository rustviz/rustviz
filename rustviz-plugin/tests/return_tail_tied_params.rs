// #775: the reference parameter a tail `if` / `match` doesn't return
// on a path. Its lifetime `'a` is the result's, so its borrow lasts
// as long as the result in the caller: no "borrow ends" on this path,
// but a dot says why. `&mut` parameters tied the same way read alike.
fn pick<'a>(c: bool, x: &'a String, y: &'a String) -> &'a String {
    if c { x } else { y }
}

fn which<'a>(n: i32, x: &'a String, y: &'a String) -> &'a String {
    match n {
        0 => x,
        _ => y,
    }
}

// A literal branch returns none of the parameters, so `x` gets the dot
// on it too.
fn lit<'a>(c: bool, x: &'a str) -> &'a str {
    if c { x } else { "none" }
}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let r = pick(true, &a, &b);
    let w = which(1, &a, &b);
    println!("{} {} {}", r, w, lit(true, &a));
}
