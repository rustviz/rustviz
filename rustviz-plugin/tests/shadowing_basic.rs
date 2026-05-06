// Shadowing: the second `let s = ...` creates a new binding with
// the same name. The first `s` goes out of scope at the shadow site
// (its resource is dropped).

fn main() {
    let s = String::from("a");
    let s = String::from("b");
    println!("{}", s);
}
