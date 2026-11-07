// #504: `for x in xs` over an owned Vec<i32> moves `xs` into the loop's
// iterator; each `x` is an i32 copied out of it, not a resource.
fn main() {
    let xs = vec![1, 2, 3];
    for x in xs {
        println!("{}", x);
    }
}
