// Tuple destructuring assignment is currently unsupported on
// the LHS of an assignment (issue #144) — the plugin used to
// panic but now gracefully skips emitting an event for this
// shape so the rest of the program renders. Pins the no-crash
// behavior until #144 lands real support.

fn main() {
    let mut a = 1;
    let mut b = 2;
    (a, b) = (b, a);
    let _ = a; // rustviz: skip
    let _ = b; // rustviz: skip
}
