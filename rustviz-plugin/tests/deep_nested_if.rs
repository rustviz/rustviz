// Three-level nested if/else mixing consumes and borrows. Each
// merge re-classifies based on its branches' end states:
//   * innermost (c3): consume vs borrow → mixed → drop dot.
//   * middle (c2): inner-merge-moved vs borrow → mixed → drop dot.
//   * outer (c1): middle-moved vs direct consume → all-moved.

fn consume(_s: String) {} // rustviz: hide
fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let c1 = true;
    let c2 = false;
    let c3 = true;
    if c1 {
        if c2 {
            if c3 {
                consume(s);
            } else {
                show(&s);
            }
        } else {
            show(&s);
        }
    } else {
        consume(s);
    }
}
