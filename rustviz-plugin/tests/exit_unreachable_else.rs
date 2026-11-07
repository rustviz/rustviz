// `unreachable!()` in an `else` panics if it ever runs, so `s` and `t`
// are dropped there (#410).

fn show(s: &String) -> usize {
    s.len()
}

fn check(s: String, ok: bool) -> usize {
    let t = String::from("t");
    if ok {
        show(&s);
    } else {
        unreachable!();
    }
    t.len()
}

fn main() {
    println!("{}", check(String::from("s"), true));
}
