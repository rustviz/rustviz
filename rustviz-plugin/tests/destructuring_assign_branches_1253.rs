// #1253: a destructuring assignment whose right-hand side is an `if` /
// `match` assigns each branch's value in that branch, as `let (x, n) = if
// …` binds it (#1007): s moves on its path only, t on the other.
fn assign_if(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let mut x = String::new();
    let mut n = 0;
    (x, n) = if c { (s, 1) } else { (t, 2) };
    println!("{} {}", x, n);
}
fn assign_match(k: i32) {
    let s = String::from("a");
    let t = String::from("b");
    let u = String::from("c");
    let mut x = String::new();
    let mut n = 0;
    (x, n) = match k {
        0 => (s, 1),
        1 => (t, 2),
        _ => (u, 3),
    };
    println!("{} {}", x, n);
}
fn assign_else_if(c: bool, d: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let mut x = String::new();
    let mut y = String::new();
    (x, y) = if c { (s, t) } else if d { (t, s) } else { (String::new(), String::new()) };
    println!("{} {}", x, y);
}
