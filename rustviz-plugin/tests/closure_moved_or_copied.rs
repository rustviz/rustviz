// #704: `let h = g;` with g a closure copies or moves the closure value;
// nothing is captured. The borrows g's captures made go with it.

// g captures only `&s`, so it is Copy: h is a copy that borrows s too,
// and s stays borrowed until h's last use.
fn copied() {
    let s = String::from("s");
    let g = || s.len();
    let h = g;
    println!("{}", g());
    println!("{}", h());
}

// g2 captures `&mut t`, so it isn't Copy: h2 takes it over, with the
// borrow; g2 no longer holds it.
fn moved_mut() {
    let mut t = String::from("t");
    let mut g2 = || t.push('!');
    let mut h2 = g2;
    h2();
    println!("{}", t);
}

// g3 owns u (a `move` closure): h3 takes it over, and dropping h3 drops u.
fn moved_owning() {
    let u = String::from("u");
    let g3 = move || u.len();
    let h3 = g3;
    println!("{}", h3());
}

fn main() {
    copied();
    moved_mut();
    moved_owning();
}
