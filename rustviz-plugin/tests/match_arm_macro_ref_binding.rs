// A match arm on `&opt` whose body is a bare macro call (#371). The
// arm's whole life is on one line: `s` borrows from `opt`, `println!`
// reads through `s`, then `s`'s borrow ends. The end used to be drawn
// on the line's first row, above the read.

fn main() {
    let opt = Some(String::from("hi"));
    match &opt {
        Some(s) => println!("{}", s),
        None => println!("none"),
    }
    println!("{:?}", opt);
}
