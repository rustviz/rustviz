// #174: `let d = c;` moves every field of `c` into `d`, including the
// Copy-typed `n` (it's part of the moved value). `d.data` now owns the
// String and drops it; `c.data` has nothing left.
struct C { data: String, n: i32 }
fn main() {
    let c = C { data: String::from("a"), n: 1 };
    let d = c;
    println!("{}", d.data);
}
