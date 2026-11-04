// The body moves `s` out (into `drop`), so `f` is `FnOnce`: calling
// it uses the closure up, and there's nothing left for `f` to drop
// at the closing brace (#271).

fn main() {
    let s = String::from("hi");
    let f = || drop(s);
    f();
}
