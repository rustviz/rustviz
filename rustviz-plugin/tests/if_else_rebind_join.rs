// Conditional join state — issue #108. Both branches consume `s`
// and rebind it from a fresh source; at the merge `s` is "bound
// here from one of the branches above" (owns a resource regardless
// of which branch ran).

fn consume(_s: String) {}

fn main() {
    let mut s = String::from("orig");
    let cond = true;
    if cond {
        consume(s);
        s = String::from("new");
    } else {
        consume(s);
        s = String::from("alt");
    }
    println!("{}", s);
}
