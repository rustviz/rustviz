// #174: `d = c;` drops the old `d.data` and moves `c.data` into `d.data`.
struct C { data: String }
fn main() {
    let mut d = C { data: String::from("old") };
    let c = C { data: String::from("new") };
    d = c;
    println!("{}", d.data);
}
