// #674: nested `if`s as the last expression. `s` and `t` are each
// returned on one path only; `u`, declared in a branch, is returned
// whenever that branch runs, so nothing is dropped at its `}`.
fn nest(a: bool, b: bool) -> String {
    let s = String::from("s");
    let t = String::from("t");
    if a {
        if b { s } else { t }
    } else if b {
        let u = String::from("u");
        u
    } else {
        String::from("none")
    }
}

fn main() {
    let c = nest(true, false);
    println!("{}", c);
}
