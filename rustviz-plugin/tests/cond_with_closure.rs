// Combination test: a closure declared inside one branch of an
// if/else captures `s` and is then called within the same arm.
// Probes how closure-capture events interact with branch-scoped
// bindings.

fn consume(_s: String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let cond = true; // rustviz: skip
    if cond {
        let f = || println!("{}", s);
        f();
    } else {
        consume(s);
    }
}
