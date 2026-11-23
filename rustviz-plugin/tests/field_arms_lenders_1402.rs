// #1402: each arm of a conditional builds the struct with its field
// pointing at a different lender. After the join d.a may point into s1
// or s2, so the borrow checker keeps both borrowed until d.a's last use
// (`s1.push('!')` before the `println!` is E0502). Both get their borrow
// back there; s1 used to have no borrow end at all.
struct One<'p> { a: &'p str }

fn main() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let d = if c { One { a: &s1 } } else { One { a: &s2 } };
    println!("{}", d.a);
}
