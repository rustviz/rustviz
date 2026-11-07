// A `return` in an `if` without `else` (#429). The `if`'s column ends
// in the drops of what the function owns at the `return` (`t`; `s` is
// returned), and the implicit `else` carries on:
// `t` is returned at the end instead.

fn pick(flag: bool, s: String) -> String {
    let t = String::from("t");
    if flag {
        return s;
    }
    t
}

fn main() {
    println!("{}", pick(true, String::from("s")));
}
