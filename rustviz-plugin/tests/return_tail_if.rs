// #674: a function whose last expression is an `if` / `else` returns
// what the branch that runs yields. `s` goes to the caller only when
// `b` is true; when it's false, `s` is still owned and dropped at the
// end of `pick`, which Rust tracks with a drop flag. A Copy `x` is
// copied to the caller in its branch and keeps its value.
fn pick(b: bool) -> String {
    let s = String::from("a");
    if b { s } else { String::new() }
}

fn num(b: bool) -> i32 {
    let x = 5;
    if b { x } else { 0 }
}

fn main() {
    let r = pick(true);
    let n = num(false);
    println!("{} {}", r, n);
}
