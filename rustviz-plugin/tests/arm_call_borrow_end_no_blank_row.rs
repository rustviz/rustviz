// #419: `r.push('x');` in an arm holds the call's borrow of `r` and the
// return of `r`'s borrow to `s` (#389); only the second is an arrow
// between variables, so it adds no blank row.
fn main() {
    let mut s = String::from("a");
    let c = true;
    if c {
        let r = &mut s;
        r.push('x');
        println!("done");
    } else {
        println!("c");
    }
}
