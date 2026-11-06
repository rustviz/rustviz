// A `match` written on one line (#117): each arm binds `s`. Events used
// to be put into arms by source line, so the first arm got all three
// binds and the arms were drawn crossing into an X.
fn main() {
    let n = 3;
    let s = match n { 0 => String::from("zero"), 1 => String::from("one"), _ => String::from("many") };
    println!("{}", s);
}
