// Destructuring a tuple variable (#189). Each name takes its own
// element: `a` takes `t.0`; `_` binds nothing, so `t.1` is not moved
// and is dropped with `t`. In `(x, .., z)` the names after `..` line
// up with the last elements.

fn main() {
    let t = (String::from("a"), String::from("b"));
    let (a, _) = t;
    let u = (String::from("x"), 1, String::from("z"));
    let (x, .., z) = u;
    println!("{} {} {} {}", a, t.1, x, z);
}
