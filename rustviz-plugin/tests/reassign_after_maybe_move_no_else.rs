// An `if` without `else` that moves `s` (#257), then a reassignment
// (#335): the old value is dropped there only if the `if` didn't run.

fn take(_s: String) {}

fn main() {
    let c = true;
    let mut s = String::from("orig");
    if c {
        take(s);
    }
    s = String::from("new");
    println!("{}", s);
}
