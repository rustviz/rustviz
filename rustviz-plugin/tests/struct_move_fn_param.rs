// #174: a by-value struct parameter owns its fields (and drops them at
// the end of `take`); the caller's `c.data` was moved along with `c`.
struct C { data: String }
fn take(c: C) { println!("{}", c.data); }
fn main() {
    let c = C { data: String::from("a") };
    take(c);
}
