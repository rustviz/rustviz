// `} else {}` right after a one-line `if` body: the empty `else` has
// a one-row column. It used to read as moving `s`, so the merge said
// "moved or dropped in every branch" (#257).
fn take(_s: String) {}

fn main() {
    let s = String::from("a");
    let c = true;
    if c {
        take(s);
    } else {}
}
