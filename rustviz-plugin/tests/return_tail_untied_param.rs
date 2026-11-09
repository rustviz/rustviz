// #775: `y` has its own elided lifetime, unrelated to the result's
// `'a`, so on the path that returns `x` the borrow of `y` really
// ends: it is drawn as usual, with no "same lifetime" dot.
fn first<'a>(c: bool, x: &'a String, y: &String) -> &'a String {
    if c { x } else { x }
}

fn pick<'a>(c: bool, x: &'a String, y: &String) -> &'a String {
    if c { x } else { println!("{}", y); x }
}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let r = first(true, &a, &b);
    let w = pick(false, &a, &b);
    println!("{} {}", r, w);
}
