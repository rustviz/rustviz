// #1140: the i32 an FnOnce closure's call returns, used inside a larger
// expression (a constructor, another call's argument, a method argument,
// a tail expression), comes from the call. The closure itself is moved
// into the call, never copied.
fn wrap(n: i32) -> i32 { n }
fn in_some() {
    let s = String::from("a");
    let f = move || { drop(s); 1 };
    let _x = Some(f());
}
fn in_call() {
    let t = String::from("t");
    let g = move || { drop(t); 2 };
    let _y = wrap(g());
}
fn in_assign() {
    let u = String::from("u");
    let h = move || { drop(u); 3 };
    let mut z = None;
    z = Some(h());
    println!("{:?}", z);
}
fn in_method_arg() {
    let w = String::from("w");
    let k = move || { drop(w); 4 };
    let mut v = Vec::new();
    v.push(k());
}
fn in_tail() -> Option<i32> {
    let q = String::from("q");
    let m = move || { drop(q); 5 };
    Some(m())
}
fn main() { in_some(); in_call(); in_assign(); in_method_arg(); in_tail(); }
