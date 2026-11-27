// #674: each arm of the tail `if` can return a reference. In
// pick_mut the selected arm reborrows through its mutable input.
// The borrow goes on in the returned reference on that path.
// Neither branch drops the value it references.
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
