// An `if` without `else` that only reads `s` changes nothing about who
// owns it, so it's still drawn inline, with no branch columns (#257
// only branches when the body moves something).
fn main() {
    let s = String::from("a");
    let c = true;
    if c {
        println!("{}", s);
    }
    println!("{}", s);
}
