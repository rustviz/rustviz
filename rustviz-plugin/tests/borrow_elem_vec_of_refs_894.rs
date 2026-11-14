// #894: borrowing an element of a collection of references (`r: &&str`)
// borrows the collection, as for `&v[1]` with `v: Vec<i32>`.
fn main() {
    let words = vec!["a", "b"];
    let r = &words[1];
    println!("{}", r);
}

fn by_index() {
    let names = vec!["x", "y"];
    let i = 1;
    let e = &names[i];
    println!("{}", e);
}

fn in_array() {
    let arr = ["m", "n"];
    let a = &arr[0];
    println!("{}", a);
}

// A copy of the stored `&str`: s points at the string literal, not into
// tags, so tags is not borrowed.
fn copied() {
    let tags = vec!["p", "q"];
    let s: &str = tags[1];
    println!("{}", s);
}
