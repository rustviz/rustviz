// #697: `return if b { p } else { q };` inside a branch. The path
// that returns p drops q at the `return`, and the other drops p. The
// fall-through path owns both until the end of the function.
fn pick(c: bool, b: bool, p: String, q: String) -> String {
    if c {
        return if b { p } else { q };
    }
    String::from("none")
}

fn main() {
    let s = pick(true, false, String::from("p"), String::from("q"));
    println!("{}", s);
}
