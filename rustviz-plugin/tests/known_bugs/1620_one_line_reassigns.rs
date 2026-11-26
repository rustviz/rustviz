// Known bug, from the closed 2.x fix PRs (fix/a-calls-1620-1645); kept as a RustViz 3 regression case.
// Issue(s): #1620 (see known_bugs/README.md for what main draws wrong).
// #1620: a `let` and two assignments on one line: three calls, three
// values moved into s, two of them dropped by the next assignment. Each
// call gets its own logo and arrow, and the dot lists each event.
fn main() {
    let mut s = String::from("a"); s = String::from("b"); s = String::from("c");
    println!("{}", s);
}
