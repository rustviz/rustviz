// Pattern guard (`pat if cond`). Verifies that:
//   * the bound pattern variable still gets a column,
//   * any borrow / read inside the guard surfaces on the right
//     timeline,
//   * arm-body events still fire normally regardless of guard outcome.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let n = 5; // rustviz: skip
    match n {
        x if x > 0 => show(&s),
        _          => show(&s),
    }
}
