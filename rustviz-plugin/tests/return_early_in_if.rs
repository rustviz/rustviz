// #318: `return s;` moves `s` to the caller. On that path `pick`
// never reaches its closing brace, so `s` is dropped there only on
// the path that skipped the `return`.
fn pick(n: i32) -> String {
    let s = String::from("early");
    if n > 0 {
        return s;
    }
    let t = String::from("late");
    t
}

fn main() {
    let r = pick(1);
    println!("{}", r);
}
