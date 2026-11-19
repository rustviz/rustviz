// #1191: returning `*b` (b a Box) moves what the Box holds to the caller,
// not b: b keeps the Box and frees its heap allocation at the end of the
// function. The same inside `return`, a branch, `Some(..)`, a tuple or an
// array, and `[*b]` in a `let`.
fn unbox(b: Box<String>) -> String {
    *b
}
fn unbox_ret(b: Box<String>) -> String {
    return *b;
}
fn unbox_if(b: Box<String>, c: bool) -> String {
    if c { *b } else { String::new() }
}
fn unbox_some(b: Box<String>) -> Option<String> {
    Some(*b)
}
fn unbox_tup(b: Box<String>) -> (String, i32) {
    (*b, 1)
}
fn unbox_n(b: Box<i32>) -> i32 {
    *b
}
fn unbox_local() -> String {
    let b = Box::new(String::from("l"));
    *b
}
fn main() {
    let s = unbox(Box::new(String::from("a")));
    println!("{}", s);
}
fn unbox_arr(b: Box<String>) -> [String; 1] {
    [*b]
}
fn unbox_arr_let(b: Box<String>) {
    let _a = [*b];
}
