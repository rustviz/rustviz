// `c` is reassigned on one branch only (#778). On the path that skips the
// `if`, `c` still points to `x` until the `println!`, so the borrow checker
// keeps `x` borrowed until then (a `x += 1;` before the `println!` is
// E0506); `x`'s borrow is drawn to that row, not to the `if`.
fn main() {
    let mut x = 8;
    let y = 9;
    let mut c = &x;
    if true {
        c = &y;
    }
    println!("{}", c);
    x += 1;
}
