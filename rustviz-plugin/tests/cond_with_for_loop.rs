// Combination test: a `for` loop inside one branch of an if/else.
// Each iteration borrows `s`; the other arm consumes it. Probes
// how the loop's per-iteration borrow events compose with the
// branch's join-state classifier.

fn consume(_s: String) {} // rustviz: hide
fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let cond = true; // rustviz: skip
    if cond {
        for _i in 0..3 {
            show(&s);
        }
    } else {
        consume(s);
    }
}
