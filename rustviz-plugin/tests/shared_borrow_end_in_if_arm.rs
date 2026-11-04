// #389, with a shared borrow: `s`'s `if` column is shared from
// `let r = &s;` to r's last use, then `s` owns its value again, on
// the same column. The return used to be drawn on the parent column,
// and the arm column stayed "shared" down to the join.
fn main() {
    let s = String::from("hi");
    let c = true;
    if c {
        println!("a");
        let r = &s;
        println!("{}", r);
        println!("b");
    } else {
        println!("c");
    }
    println!("{}", s);
}
