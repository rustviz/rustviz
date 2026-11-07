// #725: calling an `FnOnce` closure stored in a struct field moves the
// field into the call, like `f()` on a local `FnOnce` closure (#271).
// `u`, which the closure captured, comes back as the call's result `e`;
// `h` and `h.f` have nothing left to drop at the end of `main`.

struct H<F: FnOnce() -> String> {
    f: F,
}

fn main() {
    let u = String::from("u");
    let h = H { f: move || u };
    let e = (h.f)();
    println!("{}", e);
}
