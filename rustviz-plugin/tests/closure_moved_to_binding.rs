// Moving a `move` closure to another binding moves its captured
// String along: `g` drops it, `f` doesn't (#271).

fn main() {
    let s = String::from("hi");
    let f = move || println!("{}", s);
    let g = f;
    g();
}
