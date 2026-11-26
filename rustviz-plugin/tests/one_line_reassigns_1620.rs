// #1620: a `let` and two assignments on one line: three calls, three
// values moved into s, two of them dropped by the next assignment. Each
// call gets its own logo and arrow, and the dot lists each event.
fn main() {
    let mut s = String::from("a"); s = String::from("b"); s = String::from("c");
    println!("{}", s);
}
