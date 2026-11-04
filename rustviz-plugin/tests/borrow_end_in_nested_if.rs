// #389 in a nested `if`: the borrow ends in the inner `if` arm, on
// that arm's column.
fn main() {
    let mut s = String::from("hi");
    let a = true;
    let b = false;
    if a {
        if b {
            let r = &mut s;
            r.push('x');
            println!("b");
        } else {
            println!("not b");
        }
        println!("a");
    } else {
        println!("not a");
    }
    println!("{}", s);
}
