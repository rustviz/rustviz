// An `if` without `else` written on one line (#429): like any one-line
// conditional (#360) it's drawn with its own branch columns, so the
// `return` ends the `if` column in the drop of `t`, and `t` goes on
// in the implicit `else`.

fn pick(flag: bool) -> usize {
    let t = String::from("t");
    if flag { return 0; }
    t.len()
}

fn main() {
    println!("{}", pick(false));
}
