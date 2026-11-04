// A `let` initialized on only one path and used only there (#234).
// This compiles: s is never read where it might be uninitialized.
// `unused` is declared and never assigned at all.

fn main() {
    let c = true;
    let unused: i32;
    let s;
    if c {
        s = String::from("only here");
        println!("{}", s);
    } else {
        println!("no s on this path");
    }
}
