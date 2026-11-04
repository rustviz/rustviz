// A conditional written on one line (#333, #117). `s` is moved only
// when `c` holds; otherwise a runtime drop flag drops it at the end of
// `main`. Same for `t`, with an `else` on the same line. Each branch's
// events used to be picked by source line, which on one line put none
// of them in a branch, so the moves were drawn as unconditional.
fn take(_s: String) {}

fn main() {
    let s = String::from("a");
    let t = String::from("b");
    let c = true;
    if c { take(s); }
    if c { take(t); } else { println!("{}", t); }
}
