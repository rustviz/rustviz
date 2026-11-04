// An `if let` written on one line (#371): `s` borrows from `opt`,
// `println!` reads through it, and the borrow ends, all on line 6.

fn main() {
    let opt = Some(String::from("hi"));
    if let Some(s) = &opt { println!("{}", s); }
    println!("{:?}", opt);
}
