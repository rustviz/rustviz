// Regression for: macro-expanded RHS bindings (`vec![]`, `format!`,
// etc.) attributed their Bind/Acquire event to the macro's own
// source line rather than the user's `let` line. The Acquire then
// landed *after* the binding's GoOutOfScope event in time order and
// the rendered timeline column had no visible state segment / drop.
fn main() {
    let words = vec![1, 2, 3];
    let _ = words;
}
