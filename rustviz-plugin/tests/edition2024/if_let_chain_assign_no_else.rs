// #579: `z = s` in the condition moves `s` into `z` (dropping `z`'s old
// String) and lasts on the path that skips the body: there `s` may have
// been moved, and `z` holds either String.

fn main() {
    let s = String::from("hello");
    let y = Some(1);
    let mut z = String::from("old");
    if let Some(n) = y && { z = s; n > 1 } {
        println!("{}", z);
    }
    println!("{}", z);
}
