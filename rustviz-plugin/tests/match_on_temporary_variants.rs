// Other shapes that put a one-line conditional's split and join on one
// row (#459): an arm that binds the temporary's content, `if let` on a
// temporary, a value-producing `match`, and a call result as scrutinee.
fn wrap(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let a = String::from("a");
    match Some(a) { Some(x) => drop(x), None => {} }
    let b = String::from("b");
    if let Some(_) = Some(b) {} else {}
    let c = String::from("c");
    let n = match Some(c) { Some(x) => x.len(), None => 0 };
    println!("{}", n);
    let d = String::from("d");
    match wrap(d) { Some(_) => {}, None => {} }
}
