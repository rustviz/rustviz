// Binding a function to a local (#254). `f` holds a Copy function
// value that owns no resource; calling `f(a)` passes `a` to the call
// like `helper(a)` would. `helper` is called first, so it already has
// a Function entry when `let f = helper;` is visited.

fn helper(n: i32) -> i32 { n + 1 }

fn main() {
    let a = helper(1);
    let f = helper;
    println!("{} {}", a, f(a));
}
