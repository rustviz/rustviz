// Non-`move` closure that immutably borrows its upvar. Should emit
// `Immutable borrow from s to f` (matching what `let r = &s;` would).

fn main() {
    let s = String::from("hi");
    let f = || println!("{}", s);
    f();
    println!("after: {}", s);
}
