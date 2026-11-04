// A `match` scrutinee that moves a value (#450), on one line as well as
// spread over several: the move is before the arms, on every path.
fn main() {
    let v = vec![String::from("a")];
    match v.into_iter().next() {
        Some(s) => println!("{}", s),
        None => println!("none"),
    }
    let w = vec![String::from("b")];
    match w.into_iter().next() { Some(s) => println!("{}", s), None => println!("none") }
}
