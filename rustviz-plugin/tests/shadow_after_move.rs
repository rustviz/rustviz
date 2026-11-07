// Shadowing a moved variable (#253). The second `let s` declares a new
// variable `s₂`; the first `s` was moved into `t` and drops nothing at
// the end of `main`.

fn main() {
    let s = String::from("a");
    let t = s;
    let s = String::from("b");
    println!("{} {}", s, t);
}
