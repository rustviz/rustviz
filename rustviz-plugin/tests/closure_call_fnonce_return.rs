// A `move` closure that returns its capture is `FnOnce`: the call
// hands the String to `t`, which drops it; `f` drops nothing (#271).

fn main() {
    let s = String::from("a");
    let f = move || s;
    let t = f();
    println!("{}", t);
}
