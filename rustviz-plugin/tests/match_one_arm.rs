// Single-arm match (irrefutable pattern). Same treatment as
// single-arm if / if-let no-else: skip the Branch event so the
// arm's body events render inline on the parent timeline. A
// one-arm Branch would zigzag off into a lone column with no
// merge symmetry.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    match s {
        x => show(&x),
    }
}
