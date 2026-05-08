// `if cond { body }` (no else). Pre-fix the visualization
// synthesized an empty "Else" branch label even when the user
// didn't write one; now the Branch event is single-branch with
// just the "If" label.

fn main() {
    let s = String::from("hi");
    if s.len() > 0 {
        println!("non-empty: {}", s);
    }
    println!("{}", s);
}
