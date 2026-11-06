// #382: `if let Some(n) = name` moves the String out of `name` only
// when the pattern matches, at the start of the if branch's column.
// The second `if let` has no `else`: the implicit empty `else` keeps
// the String in `opt`.
fn consume(_o: String) {}

fn main() {
    let name: Option<String> = Some(String::from("Ann"));
    if let Some(n) = name {
        consume(n);
    } else {
        println!("nobody");
    }
    let opt = Some(String::from("b"));
    if let Some(s) = opt {
        consume(s);
    }
}
