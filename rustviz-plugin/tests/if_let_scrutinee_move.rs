// The scrutinee of an `if let` is evaluated before either branch is
// chosen (#450): `v.into_iter()` moves `v` on the `if let` line, on
// every path. It was drawn inside the first branch, which returns, so
// the merge ignored it and `v`'s scope end claimed its resource was
// dropped.
fn first(v: Vec<String>) -> usize {
    let t = String::from("t");
    if let Some(s) = v.into_iter().next() {
        return s.len();
    } else {
        println!("none");
    }
    t.len()
}

fn main() {
    println!("{}", first(vec![String::from("a")]));
}
