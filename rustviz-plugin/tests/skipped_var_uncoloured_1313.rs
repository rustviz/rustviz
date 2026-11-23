// #1313: a `// rustviz: skip` variable has no column, so the code panel
// must not colour it (the colour would match nothing on the timeline).
// Variables with a column stay coloured.
struct P { a: String, b: String }

fn main() {
    let mut s = String::from("hello");
    let mut q = String::from("skipped"); // rustviz: skip
    let cond = true; // rustviz: skip
    let n = 3; // rustviz: skip
    let (u, v) = (String::from("u"), String::from("v")); // rustviz: skip
    let p = P { a: String::from("a"), b: String::from("b") };
    let r1 = &s;
    println!("{}", r1);
    q.push('x');
    if cond {
        s.push('!');
    }
    let c = || n + 1;
    println!("{} {} {}", c(), u, v);
    println!("{}", p.a);
}
