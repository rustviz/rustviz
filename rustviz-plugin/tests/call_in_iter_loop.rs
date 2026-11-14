// #900: a call that borrows v inside a `for` loop whose iterator borrows
// v (`v.iter().enumerate()`, `w.iter().map(..)`, `s.chars()`, `&v` with
// a pattern that copies) gives back only its own borrow: the loop's
// iterator still borrows v until the loop's `}`.
fn main() {
    let v = vec![String::from("a"), String::from("b")];
    for (i, a) in v.iter().enumerate() {
        println!("{} {} {}", i, a, v.len());
    }
    let w = vec![1, 2, 3];
    for x in w.iter().map(|n| n + 1) {
        println!("{} {}", x, w.len());
    }
    let s = String::from("hi");
    for c in s.chars() {
        println!("{} {}", c, s.len());
    }
    let u = vec![4, 5];
    for &y in &u {
        println!("{} {}", y, u.len());
    }
    let t = vec![7, 8];
    for z in t.iter().take(t.len()) {
        println!("{}", z);
    }
}
