// A match arm moves `s`, the others don't (#335). `t` is moved in every
// arm: its reassignment drops nothing. `u` is moved in none: its
// reassignment drops the old value.

fn take(_s: String) {}

fn main() {
    let n = 2;
    let mut s = String::from("s0");
    let mut t = String::from("t0");
    let mut u = String::from("u0");
    match n {
        0 => {
            take(s);
            take(t);
        }
        _ => {
            take(t);
            println!("{}", u);
        }
    }
    s = String::from("s1");
    t = String::from("t1");
    u = String::from("u1");
    println!("{} {} {}", s, t, u);
}
