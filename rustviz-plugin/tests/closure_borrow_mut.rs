// Non-`move` closure that mutably borrows its upvar. Should emit
// `Mutable borrow from s to f` (matching `let r = &mut s;`).

fn main() {
    let mut s = String::from("hi");
    let mut f = || s.push_str("!");
    f();
    println!("{}", s);
}
