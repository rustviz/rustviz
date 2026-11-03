// #174: moving a whole struct (here into `drop`) moves its fields too,
// so nothing is left for `c.data` to drop at the end of main.
struct C { data: String }
fn main() {
    let c = C { data: String::from("a") };
    drop(c);
}
