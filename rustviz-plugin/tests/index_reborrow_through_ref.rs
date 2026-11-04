// #407: borrowing an element or slice through a reference is a
// reborrow. `&mut rv[0]` is `IndexMut::index_mut(&mut *rv, 0)`: it
// borrows through rv, the same way `&mut *rv` does, and so do
// `add(&mut rv[0])`, `rv[0].push(..)` and `&r[1..]`. Indexing an
// owner (`&mut w[0]`) is a plain borrow of w.
fn add(s: &mut String) {
    s.push('!');
}

fn main() {
    let mut v = vec![String::new()];
    let rv = &mut v;
    let e = &mut rv[0];
    e.push('x');
    add(&mut rv[0]);
    rv[0].push('y');
    let mut w = vec![String::new()];
    let f = &mut w[0];
    f.push('z');
    let s = String::from("hello");
    let r = &s;
    let sl = &r[1..];
    println!("{} {} {}", sl, v.len(), w.len());
}
