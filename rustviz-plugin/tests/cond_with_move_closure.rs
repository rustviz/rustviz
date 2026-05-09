// Combination test: a `move` closure declared inside one branch
// of an if/else takes ownership of `s`. The other arm consumes
// `s` directly. Both arms end without `s` → all-moved merge.

fn consume(_s: String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let cond = true; // rustviz: skip
    if cond {
        let f = move || println!("captured: {}", s);
        f();
    } else {
        consume(s);
    }
}
