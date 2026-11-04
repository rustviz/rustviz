// A `return` after a nested if/else in the same branch (#410): the
// nested conditional used to end the "inside a branch" state, so the
// `return` dropped nothing. `s` and `t` are dropped at it.

fn pick(s: String, a: bool, b: bool) -> usize {
    let t = String::from("t");
    if a {
        if b {
            drop(t.clone());
        } else {
            drop(s.clone());
        }
        return 0;
    } else {
        drop(t.clone());
    }
    s.len() + t.len()
}

fn main() {
    println!("{}", pick(String::from("s"), true, false));
}
