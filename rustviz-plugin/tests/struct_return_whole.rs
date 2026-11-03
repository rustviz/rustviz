// #174: returning `c` moves `c.data` to the caller; `x.data` receives it.
struct C { data: String }
fn make() -> C {
    let c = C { data: String::from("a") };
    c
}
fn main() {
    let x = make();
    println!("{}", x.data);
}
