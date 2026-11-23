// #1327: `r = match &o { Some(x) => x, None => &t };` assigns r a copy of
// x, a reference into o: r borrows o (not x) until its last use.
fn main() {
    let o = Some(String::from("hi"));
    let t = String::from("t");
    let mut r = &t;
    println!("{}", r);
    r = match &o { Some(x) => x, None => &t };
    println!("{}", r);
}
