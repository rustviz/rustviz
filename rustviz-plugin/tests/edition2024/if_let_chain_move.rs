// A let-chain with no `else` that moves out of `x` (#198). The move
// happens as soon as `let Some(a) = x` matches, whether or not the rest
// of the chain holds, so `x` has nothing left to drop at the end. Before
// the fix the move wasn't drawn and `x` was said to drop its String.

fn consume(_o: String) {}

fn main() {
    let x = Some(String::from("a"));
    if let Some(a) = x && a.len() > 0 {
        consume(a);
    }
}
