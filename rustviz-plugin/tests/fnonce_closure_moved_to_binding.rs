// `let g = f;` moves the closure, so `f` has nothing to drop at the
// end of `main` and `g` drops the captured String (#271).

fn main() {
    let s = String::from("hi");
    let f = move || s;
    let g = f;
    drop(g);
}
