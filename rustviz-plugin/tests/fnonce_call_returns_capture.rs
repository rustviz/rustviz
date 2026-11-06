// A `move` closure that returns its capture is `FnOnce`: the call
// moves `f`, and the String comes out as the call's result (#271).

fn main() {
    let s = String::from("a");
    let f = move || s;
    let t = f();
    println!("{}", t);
}
