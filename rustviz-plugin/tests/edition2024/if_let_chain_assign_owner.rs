// #579: `z = a` moves the String out of `a` into `z` and drops `z`'s
// old String, before `z.len() > 3` is checked. In the else branch `z`
// holds either its old String (`y` was `None`) or the new one (the
// length check failed); either way it holds one, so it's dropped at the
// end without a drop flag. `y` may have been moved into `a`, as in #281.

fn main() {
    let y = Some(String::from("hi"));
    let mut z = String::from("old");
    if let Some(a) = y && { z = a; z.len() > 3 } {
        println!("long {}", z);
    } else {
        println!("short {}", z);
    }
}
