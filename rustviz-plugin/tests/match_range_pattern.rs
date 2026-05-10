// Range patterns in match arms (`a..=b`). Pre-fix the visitor's
// `PatKind::Range` arm at visitor.rs:1591 silently dropped the
// pattern; this fixture verifies that the surrounding match still
// renders coherently (each arm picks up the consume / borrow
// events normally) and the merge classifier sees the arm endings.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let n = 5; // rustviz: skip
    match n {
        0..=4 => show(&s),
        5..=9 => show(&s),
        _     => show(&s),
    }
}
