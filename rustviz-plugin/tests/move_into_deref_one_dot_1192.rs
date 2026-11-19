// #1192: `*r = t;` moves t into what r points to. On p's column the end
// of r's borrow and the drop of p's old value are on the same line; the
// move's arrow added a row below, and the drop was drawn there, as a
// second dot, instead of on the same dot as the end of the borrow (as
// `*r = P { .. }` already did).
struct P { name: String }

fn main() {
    let t = P { name: String::from("t") };
    let mut p = P { name: String::new() };
    let r = &mut p;
    *r = t;
    println!("{}", p.name);
}

fn string() {
    let mut s = String::from("s");
    let u = String::from("u");
    let q = &mut s;
    *q = u;
    println!("{}", s);
}
