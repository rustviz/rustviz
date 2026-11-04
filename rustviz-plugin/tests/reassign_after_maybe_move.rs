// `s` is moved in the `if` branch only. At the reassignment, Rust drops
// the old value only if the `else` branch ran (#335).

fn take(_s: String) {}

fn main() {
    let c = true;
    let mut s = String::from("orig");
    if c {
        take(s);
    } else {
        println!("no");
    }
    s = String::from("new");
    println!("{}", s);
}
