// #1518: a `loop` whose body always returns before its `break`: the break
// is never reached, so the loop never ends and nothing after it runs
// (rustc warns "unreachable statement"). s is dropped at the `return`.
#![allow(unreachable_code)]
fn main() {
    let s = String::from("hey");
    let r = &s;
    loop {
        return;
        break;
    }
    let r2 = r;
}
