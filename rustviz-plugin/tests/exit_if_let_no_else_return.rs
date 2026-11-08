// `if let` without `else` whose body returns (#429): the `return`
// drops `t` and `s` (the value the pattern moved out of `opt`). When
// the pattern doesn't match, nothing is moved and the function goes
// on.

fn first(opt: Option<String>) -> usize {
    let t = String::from("t");
    if let Some(s) = opt {
        return t.len() + s.len();
    }
    t.len()
}

fn main() {
    println!("{}", first(Some(String::from("a"))));
}
