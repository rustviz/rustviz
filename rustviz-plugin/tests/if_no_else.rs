// `if cond { body }` (no else) whose body moves `s`. `s` is moved
// only when `cond` holds; otherwise it's still owned and dropped at
// scope end (a runtime drop flag decides). Drawn like an `if` with an
// empty `else` (#257); it used to show an unconditional move.

fn consume(_s: String) {}

fn main() {
    let s = String::from("hi");
    let cond = true;
    if cond {
        consume(s);
    }
}
