// `drop(x)` is where x's value is dropped: the move into it says so.
// A Copy value has nothing to drop, and a user function that happens to
// be named `drop` isn't `std::mem::drop`.
mod mine {
    pub fn drop(s: String) -> usize {
        s.len()
    }
}

fn main() {
    let s = String::from("s");
    drop(s);
    let n = 5;
    drop(n);
    let v = vec![1];
    std::mem::drop(v);
    let t = String::from("t");
    mine::drop(t);
}
