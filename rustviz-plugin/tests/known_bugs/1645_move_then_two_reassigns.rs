// Known bug, from the closed 2.x fix PRs (fix/a-calls-1620-1645); kept as a RustViz 3 regression case.
// Issue(s): #1645 (see known_bugs/README.md for what main draws wrong).
// #1645: `drop(s)` empties s, the first assignment gives it "b" (nothing
// to drop), and the second drops "b" before storing "c".
fn main() {
    let mut s = String::from("a");
    drop(s); s = String::from("b"); s = String::from("c");
    println!("{}", s);
}
