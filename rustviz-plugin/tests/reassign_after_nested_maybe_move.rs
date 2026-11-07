// `s` is moved on one path of a nested conditional and kept on the
// others, then reassigned (#335). Both merges and the reassignment say
// the old value is dropped at the reassignment only if it's still there.

fn take(_s: String) {}

fn main() {
    let a = true;
    let b = false;
    let mut s = String::from("x");
    if a {
        if b {
            take(s);
        } else {
            println!("{}", s);
        }
    } else {
        take(s);
    }
    s = String::from("y");
    println!("{}", s);
}
