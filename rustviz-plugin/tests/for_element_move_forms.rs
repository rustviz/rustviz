// #850: the other ways to write a `for` over owned Strings. An array moves
// into its iterator like a Vec; `into_iter()` / `drain(..)` / `enumerate()`
// heads are calls, so no arrow runs from the collection to `w`; a loop over
// `&kept` borrows and moves nothing.
fn consume(_s: String) {}

fn main() {
    let more = vec![String::from("a")];
    for w in more.into_iter() {
        consume(w);
    }
    let pairs = vec![String::from("p")];
    for (i, w) in pairs.into_iter().enumerate() {
        println!("{}", i);
        consume(w);
    }
    let mut drained = vec![String::from("d")];
    for w in drained.drain(..) {
        consume(w);
    }
    let arr = [String::from("x"), String::from("y")];
    for w in arr {
        consume(w);
    }
    let kept = vec![String::from("k")];
    for w in &kept {
        println!("{}", w);
    }
}
