// `if cond { body }` (no else). Pre-fix the visualization
// synthesized an empty "Else" branch label even when the user
// didn't write one; now the Branch event is single-branch with
// just the "If" label, and `s`'s join state at the merge reflects
// the implicit-untouched else (still owned in the no-else path,
// so MovedAfter — not BoundHere).

fn consume(_s: String) {}

fn main() {
    let s = String::from("hi");
    let cond = true;
    if cond {
        consume(s);
    }
}
