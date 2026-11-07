// #674: a `match` as the function's last expression: each arm's value
// goes to the caller in that arm. `s` and `t` are each returned by one
// arm and dropped at the end of `choose` on the paths through the
// others.
fn choose(n: u32) -> String {
    let s = String::from("one");
    let t = String::from("two");
    match n {
        1 => s,
        2 => t,
        _ => String::from("many"),
    }
}

fn main() {
    let c = choose(1);
    println!("{}", c);
}
