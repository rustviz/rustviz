// An exit in an `if` without `else` inside another one (#429): the
// inner body always exits, so its column ends in the drops; the outer
// body doesn't always exit, so it gets none of its own.

fn check(a: bool, b: bool) -> usize {
    let t = String::from("t");
    let u = String::from("u");
    if a {
        if b {
            return 1;
        }
        println!("{}", u);
    }
    t.len()
}

fn main() {
    println!("{}", check(true, false));
}
