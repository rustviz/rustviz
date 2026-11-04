// #388: hovering a branch column shows the tooltip of the segment under
// the mouse. `s`'s `if` column has an "owner" stretch, then a "shared"
// one from `let r = &s;` on. One hover polygon used to cover the whole
// column and show "s is the owner of the resource" over the shared
// stretch too.
fn main() {
    let s = String::from("hi");
    let c = true;
    if c {
        println!("a");
        let r = &s;
        println!("{}", r);
        println!("b");
    } else {
        println!("c");
    }
    println!("{}", s);
}
