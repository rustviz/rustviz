// Both arms consume `s` directly. Merge classifies as all-moved
// (every recorded branch ends without the resource and the if/else
// has no implicit untouched arm), so the merge dot's tooltip
// pins the every-branch wording rather than the may-have-been or
// implicit-drop wording.

fn consume(_s: String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let cond = true;
    if cond {
        consume(s);
    } else {
        consume(s);
    }
}
