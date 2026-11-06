// An `FnOnce` closure called only on one path is maybe-moved at the
// end of `main`: the drop flag decides whether `f` still drops its
// capture (#271).

fn main() {
    let s = String::from("hi");
    let f = || drop(s);
    let c = std::env::args().count() > 1;
    if c {
        f();
    }
}
