// #645: `r` borrows `t` inside the branch that returns. The `return`
// drops `t`'s String; `r`'s borrow ends first, at its last use on the
// same line. The dot must not say "No resource is dropped".
fn f(c: bool) -> usize {
    let t = String::from("t");
    if c {
        let r = &t;
        return r.len();
    } else {
        println!("no");
    }
    t.len()
}

fn main() {
    println!("{}", f(true));
}
