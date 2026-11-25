// #1519: `for e in v` with `v: Vec<&str>` takes v's elements by value:
// each e is one of the references v stored (here to string literals). e
// borrows nothing of v, so nothing goes back to v when e ends.
fn main() {
    let v = vec!["x", "y"];
    for e in v {
        println!("{}", e);
    }
    let w = vec!["z"];
    for f in w.into_iter() {
        println!("{}", f);
    }
    let arr = ["a", "b"];
    for g in arr {
        println!("{}", g);
    }
    let words = vec![String::from("w")];
    for h in words.iter() {
        println!("{}", h);
    }
}
