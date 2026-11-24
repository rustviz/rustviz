// #1423: `b = Box::new(*b);` moves the String out of the Box first, then
// the assignment frees only the old Box's heap allocation. The corpus
// check `drop-of-moved-value-at-reassign` accepts that dot.
fn run(s: String) -> Option<usize> {
    let mut b = Box::new(s);
    b = Box::new(*b);
    println!("{}", b);
    Some(0)
}

fn main() {
    run(String::from("a"));
}
