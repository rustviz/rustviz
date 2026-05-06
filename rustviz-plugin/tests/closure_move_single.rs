// `move` closure with a single captured upvar. Should emit
// `Move from s to f` at the let line so the user sees ownership
// of `s` transferring into the closure.

fn main() {
    let s = String::from("hi");
    let f = move || println!("{}", s);
    f();
}
