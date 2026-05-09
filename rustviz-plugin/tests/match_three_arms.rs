// Three-arm match mixing a consume, a borrow, and a borrow. The
// merge classifies as mixed (consume + alive arms) → drop dot
// with the implicit-drop wording. Pinned in the corpus to guard
// against regressions in N>2 branch placement / classification.

fn consume(_s: String) {} // rustviz: hide
fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let n = 1;
    match n {
        0 => consume(s),
        1 => show(&s),
        _ => show(&s),
    }
}
