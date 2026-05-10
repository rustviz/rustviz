// Closure body events surface on captured-upvar timelines at the
// closure's call site (#133). Before this, `show(&s)` inside the
// closure body produced no event on `s`'s column even though the
// borrow was exercised at call time.

fn show(_s: &String) {}

fn main() {
    let s = String::from("hi");
    let f = || show(&s);
    f();
}
