// #1267: an `if` / `match` wrapped in a constructor inside a tuple or
// struct literal (`(Some(if …), 1)`), or as an element of a tuple passed to
// a call (`take((if …, 1))`), gives its value from each branch, in that
// branch: d moves on the then-path only, e on the else-path.
struct W {
    s: Option<String>,
    k: i32,
}
fn take<T>(_t: T) {}
fn ctor_elem(c: bool) {
    let d = String::from("d");
    let e = String::from("e");
    let _t = (Some(if c { d } else { e }), 1);
}
fn ctor_field(o: Option<i32>) {
    let f = String::from("f");
    let g = String::from("g");
    let _w = W { s: Some(match o { Some(_) => f, None => g }), k: 2 };
}
fn arg_tuple(c: bool) {
    let h = String::from("h");
    let i = String::from("i");
    take((if c { h } else { i }, 1));
}
fn method_arg_tuple(c: bool) {
    let j = String::from("j");
    let k = String::from("k");
    let mut v: Vec<(String, i32)> = Vec::new();
    v.push((match c { true => j, false => k }, 1));
    println!("{}", v.len());
}
fn main() {}
