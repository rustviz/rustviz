// Moving one element out of a tuple (#189). `t.0` and `t.1` have
// columns of their own, like a struct's fields: `first` takes the
// String out of `t.0`, and `t.1` keeps its String, which is dropped
// with `t` at the end of `main`.

fn main() {
    let t = (String::from("a"), String::from("b"));
    let first = t.0;
    println!("{}", first);
}
