// #674: references returned by the branches of a tail `if`. A shared
// reference is copied to the caller; a `&mut` one is moved, on that
// path only. Neither owns anything, so nothing is dropped on the
// paths that didn't return it.
fn longer<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}

fn pick_mut<'a>(b: bool, m: &'a mut String, k: &'a mut String) -> &'a mut String {
    if b { m } else { k }
}

fn main() {
    let a = String::from("ab");
    let r = longer(&a, "c");
    let mut p = String::from("p");
    let mut q = String::from("q");
    let w = pick_mut(true, &mut p, &mut q);
    w.push('!');
    println!("{} {}", r, w);
}
