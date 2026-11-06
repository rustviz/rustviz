// #389: `r` is declared inside the `if` arm, so its borrow of `s` ends
// there: `s` gets its value back on its `if` column, at `r.push('x')`
// (r's last use). It used to get it back on its parent column, which
// then ran beside the arm's code as if the `if` weren't there.
fn main() {
    let mut s = String::from("hi");
    let c = true;
    if c {
        println!("a");
        let r = &mut s;
        r.push('x');
        println!("b");
    } else {
        println!("c");
    }
    println!("{}", s);
}
