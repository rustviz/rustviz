// A Copy value's scope end reads "x goes out of scope" however the value
// was used (#793): it has no resource to drop, or to have moved away.
// `c.n` and `pair.1` are Copy fields of structs/tuples that were moved
// whole; on main they said "No resource is dropped." like the String
// fields do, which suggests a resource was involved.
struct C { data: String, n: i32 }

fn take(pair: (String, i32)) -> usize {
    pair.0.len()
}

fn main() {
    let x = 5;
    let c = C { data: String::from("a"), n: 1 };
    let d = c;
    let pair = (String::from("b"), 2);
    let len = take(pair);
    println!("{} {} {} {}", d.data, d.n, x, len);
}
