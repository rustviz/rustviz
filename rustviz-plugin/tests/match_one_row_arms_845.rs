// Match arms of one row with no events for `s` (#845): each is solid over
// its own row (the first, a middle one, and the last), dashed beside the
// other arms' rows.
fn main() {
    let n = 3;
    let s = String::from("hi");
    match n {
        0 => println!("zero"),
        1 => println!("one"),
        2 => println!("two"),
        _ => drop(s),
    }
}
