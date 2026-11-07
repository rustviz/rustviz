// `?` in a match arm (#636): `Some(s) =>` moved the String out of `o`
// into `s`, so when the `?` returns early `f` drops `s`, `v` and `t`,
// and nothing of `o`. Before, the arm's columns dropped the `?`'s
// events; drawn, they'd have named `o` and not `s`, because the arm's
// pattern bindings were left out of what the `?` saw.

fn f(o: Option<String>, v: Vec<String>) -> Option<usize> {
    let t = String::from("t");
    match o {
        Some(s) => {
            let first = v.first()?;
            println!("{} {} {}", s, first, t);
        }
        None => println!("{}", t),
    }
    Some(1)
}

fn main() {
    f(Some(String::from("a")), vec![]);
}
