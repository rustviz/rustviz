// A closure that's never called never runs its body (#133): nothing
// happens to `s` after the capture except the borrow ending.

fn main() {
    let s = String::from("hi");
    let _f = || println!("{}", s);
    println!("{}", s);
}
