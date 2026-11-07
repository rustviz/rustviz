// Destructuring assignment `(a, b) = (b, a);` (Rust 1.59+). rustc
// lowers it to `{ let (lhs, lhs) = (b, a); a = lhs; b = lhs; }`. The
// compiler's `lhs` temporaries get no column; each variable is drawn
// as assigned from the part of the right-hand side it receives. In
// the String swap both values move out before either arrives, so
// nothing is dropped. (#151)
fn pair() -> (String, i32) {
    (String::from("p"), 7)
}

fn main() {
    let mut x = 1;
    let mut y = 2;
    (x, y) = (y, x);
    let mut a = String::from("a");
    let mut b = String::from("b");
    (a, b) = (b, a);
    let mut s = String::new();
    let mut n = 0;
    (s, n) = pair();
    println!("{} {} {} {} {} {}", x, y, a, b, s, n);
}
