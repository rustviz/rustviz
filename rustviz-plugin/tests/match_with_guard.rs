// Pattern guard (`pat if cond`). Verifies that:
//   * the bound pattern variable still gets a column,
//   * arm-body events still fire normally regardless of guard outcome.
// The guard expression itself (`x > 0`) is NOT visualized: the
// visitor never walks `arm.guard`, so reads / borrows / moves inside
// a guard produce no events. This fixture only pins that a guarded
// arm doesn't crash or disturb the rest of the match.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let n = 5; // rustviz: skip
    match n {
        x if x > 0 => show(&s),
        _          => show(&s),
    }
}
