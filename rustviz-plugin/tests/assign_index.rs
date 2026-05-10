// Index assignment (`v[i] = expr`) is currently unsupported on
// the LHS of an assignment (issue #144) — the plugin used to
// panic but now gracefully skips this shape. Pins the no-crash
// behavior; the assignment itself is invisible in the timeline
// until #144 is implemented.

fn main() {
    let mut v = vec![1, 2, 3]; // rustviz: skip
    v[0] = 9;
    let _ = v.len(); // rustviz: skip
}
