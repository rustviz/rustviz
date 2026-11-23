// `for x in v.iter_mut()` / `v.iter()`: each x is a reference into v,
// which v lends as for `for x in &mut v`: a borrow from v to x on the
// `for` line, and x's return at the loop's end (#1347).
struct P { items: Vec<String> }
fn main() {
    let mut v = vec![String::from("a")];
    for x in v.iter_mut() {
        x.push('?');
    }
    for y in v.iter() {
        if y.len() > 3 { break; }
    }
    let q = P { items: vec![String::from("b")] };
    for s in q.items.iter() {
        println!("{}", s);
    }
    for z in &mut v {
        z.push('!');
    }
    println!("{:?}", v);
}
