// #318: `0 => return s` moves `s` to the caller in that arm only; the
// other arm reaches the code after the `match`, where `s` is still
// owned.
fn pick(n: i32) -> String {
    let s = String::from("a");
    let k = match n {
        0 => return s,
        m => m,
    };
    println!("{} {}", s, k);
    s
}

fn main() {
    let r = pick(1);
    println!("{}", r);
}
