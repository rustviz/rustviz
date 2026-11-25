// #1521: a reference re-pointed by copying a reference of the other kind.
// After `r = r2;`, r holds r2's shared loan, which ends as a shared one
// (the invariant check used to call that a borrow-kind mismatch).
fn downgrade(s: &mut String) -> &String {
    s
}

fn used_after() {
    let mut s = String::from("x");
    let mut r = downgrade(&mut s);
    let r2 = &s;
    r = r2;
    println!("{}", r);
}

fn last_use() {
    let mut s = String::from("x");
    let mut r = downgrade(&mut s);
    let r2 = &s;
    r = r2;
}

fn main() {
    used_after();
    last_use();
}
