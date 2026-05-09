// Combination test: an if/else inside a for loop body. The body's
// branch event should fire per iteration; in our visualization
// the loop renders as a single iteration so we just need the
// branch to compose with the loop body.

fn consume(_s: String) {} // rustviz: hide
fn show(_s: &String) {} // rustviz: hide

fn main() {
    let xs = [String::from("a"), String::from("b")];
    let cond = true; // rustviz: skip
    for x in &xs {
        if cond {
            show(x);
        } else {
            show(x);
        }
    }
    let s = String::from("end");
    consume(s);
}
