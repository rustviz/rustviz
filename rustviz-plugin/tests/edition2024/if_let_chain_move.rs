// A let-chain whose last `let` moves out of `x` (#198). That `let`'s
// bindings happen only when the whole condition holds, so the move is
// drawn in the if branch, and after the `if` `x` is dropped only if the
// body didn't run. (A move by an earlier part also happens when a later
// part fails: if_let_chain_no_else_move, #281.)

fn consume(_o: String) {}

fn main() {
    let n = 3;
    let x = Some(String::from("a"));
    if n > 0 && let Some(a) = x {
        consume(a);
    }
}
