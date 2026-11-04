// Operands that already are references (`x == y` with `x: &String`) or
// are reached through `*` are read through; Rust doesn't borrow an
// owner for them. A written `&b` is an explicit borrow.
fn same(x: &String, y: &String) -> bool {
    let e = x == y;
    let l = *x < *y;
    e && l
}

fn main() {
    let a = String::from("x");
    let b = String::from("y");
    let r = &a;
    let eq = r == &b;
    let s = same(&a, &b);
    println!("{} {} {}", eq, s, a);
}
