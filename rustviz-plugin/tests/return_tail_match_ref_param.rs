// #742: a shared-reference parameter returned from an arm of a tail
// `match`: the arm that returns it hands a copy of the reference to
// the caller, so its borrow doesn't end there.
fn pick<'a>(n: u32, x: &'a String, y: &'a String) -> &'a String {
    match n {
        0 => x,
        _ => y,
    }
}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let r = pick(0, &a, &b);
    println!("{}", r);
}
