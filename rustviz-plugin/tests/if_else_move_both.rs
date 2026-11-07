// Both arms consume `s` directly. Merge classifies as all-moved
// (every recorded branch ends without the resource and the if/else
// has no implicit untouched arm), so `s`'s column ends with the arms
// (no join dot, #694) and its scope-end dot pins the every-branch
// wording rather than the may-have-been or implicit-drop wording.

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
