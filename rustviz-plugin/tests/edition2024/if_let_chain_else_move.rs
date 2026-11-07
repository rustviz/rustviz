// #281: `let Some(a) = x` moves the String out of `x` as soon as it
// matches. If `a.len() > 5` is then false, the else branch runs with
// `x` already moved (rustc: "borrow of partially moved value" if the
// else uses `x`), and `a` is dropped, with the String, as the condition
// fails. If `x` is `None`, the else branch runs with `x` untouched. So
// the else column starts with `x` maybe-moved, and `x` is dropped at
// the end only if neither path moved it. (This was refused; before
// #198's fix, main drew no move and said `x`'s resource is dropped.)

fn consume(_o: String) {}

fn main() {
    let x = Some(String::from("a"));
    if let Some(a) = x && a.len() > 5 {
        consume(a);
    } else {
        println!("short");
    }
}
