// #1360 in a let chain: the scrutinee temporary of the chain's `let`, which
// the `return` in the matched block drops, forks at the `if`. The return's
// leg ends in that drop; the matched block's other path drops it at the
// block's end, the else path before the `else` block.
fn make() -> Vec<String> {
    vec![String::from("a")]
}

fn chain(n: usize) -> usize {
    if let Some(s) = make().pop() && n > 1 {
        if n == 5 {
            return 0;
        }
        s.len()
    } else {
        2
    }
}

fn main() {
    chain(1);
}
