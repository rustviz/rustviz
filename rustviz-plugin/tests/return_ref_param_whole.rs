// #754: a shared reference parameter that is the fn's whole result,
// as the tail expression or with `return`. The reference is copied to
// the caller and the borrow carries on there (through `r`), so x's
// borrow doesn't end inside `id`.
fn id(x: &String) -> &String {
    x
}

fn main() {
    let s = String::from("a");
    let r = id(&s);
    println!("{}", r);
}
