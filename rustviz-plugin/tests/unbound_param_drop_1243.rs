// #1243: a parameter whose pattern binds no name (`_: String`, `(_, _)`)
// still owns the caller's argument until the function returns. It gets a
// `(parameter)` column so its drop is stated. Nothing for a Copy value or
// a reference (`_: i32`, `_: &String`): nothing is dropped.
struct P { name: String, n: i32 }
fn f(_: String, (_, _): (String, String)) {
    println!("x");
}
fn g(_: i32, _: &String, P { .. }: P) {}
fn h(s: String, _: Vec<String>) -> String { s }
fn main() {
    let v = vec![String::from("a")];
    v.into_iter().for_each(|_| ());
    f(String::from("a"), (String::from("b"), String::from("c")));
}
