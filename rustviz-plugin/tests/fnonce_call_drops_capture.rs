// Calling an `FnOnce` closure moves it into the call (#271): the body
// consumes `s`, which `f` captured by value, so `f` drops nothing at
// the end of `main`.

fn main() {
    let s = String::from("hi");
    let f = || drop(s);
    f();
}
