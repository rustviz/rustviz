// A variable created, borrowed by a call and dropped on one line: its
// merged dot carries the call icon, which covers the dot, so the drop is
// marked by a triangle just below the dot rather than inside it.
fn show(_s: &String) {}

fn main() {
    { let s = String::from("a"); show(&s); }
}
