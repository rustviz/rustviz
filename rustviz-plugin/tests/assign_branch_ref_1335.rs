// #1335: `r = if c { let y = &a; y } else { &b };` assigns r a copy of
// the reference y made in the arm: y's borrow ends with y at the arm's
// end, and r gives a (or b) back at its own last use.
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut r = &b;
    println!("{}", r);
    r = if a.len() > 0 { let y = &a; y } else { &b };
    println!("{}", r);
    let c = String::from("c");
    let o: Option<&String> = Some(&c);
    r = match o { Some(x) => { let z = x; z } None => &b };
    println!("{}", r);
}
