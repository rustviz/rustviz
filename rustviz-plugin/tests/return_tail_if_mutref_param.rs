// #742: a `&mut` parameter returned from one branch of a tail `if`
// goes to the caller; its borrow does not end there. Only the path
// that doesn't return it ends it.
fn pick<'a>(c: bool, m: &'a mut String, n: &'a mut String) -> &'a mut String {
    if c { m } else { n }
}

fn main() {
    let mut p = String::from("p");
    let mut q = String::from("q");
    let w = pick(true, &mut p, &mut q);
    w.push('!');
    println!("{}", w);
}
