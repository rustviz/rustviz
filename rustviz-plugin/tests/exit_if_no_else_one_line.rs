// An `if` without `else` written on one line is still drawn as if its
// body always ran (#333), so the `return`'s drop isn't drawn: it would
// end `t`'s column for the path that doesn't return.

fn pick(flag: bool) -> usize {
    let t = String::from("t");
    if flag { return 0; }
    t.len()
}

fn main() {
    println!("{}", pick(false));
}
