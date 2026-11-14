// #940: a let-chain's condition that moves a Box's contents (`let w =
// *e;`), not the Box. e keeps its heap allocation on every path; if the
// condition failed before the move, the old String is still in the Box
// and is dropped at the next `*e = …` (e), at `f = …` with the old Box
// (f), or at the scope end (g). The joins said "e was moved in at least
// one branch above" and that e's resource is dropped at its scope end.
// Checked with a `Drop`-printing type.

fn main() {
    let o = Some(3);
    let mut e = Box::new(String::from("a"));
    if let Some(x) = o && x > 5 && { let w = *e; drop(w); true } {
        println!("y");
    }
    *e = String::from("b");
    println!("{}", e);

    let mut f = Box::new(String::from("f"));
    if let Some(x) = o && x > 5 && { let w = *f; drop(w); true } {
        println!("y");
    }
    f = Box::new(String::from("f2"));
    println!("{}", f);

    let g = Box::new(String::from("g"));
    if let Some(x) = o && x > 5 && { let w = *g; drop(w); true } {
        println!("y");
    }
}
