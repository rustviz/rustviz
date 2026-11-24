// #1405: a pattern that takes apart the value of an `if` / `match`
// (`let One { a } = if c { One { a: &s1 } } else { .. };`) gives each
// reference it binds what that branch's value borrows: `a` may point
// into s1 or s2, so both stay borrowed until a's last use.
struct One<'p> { a: &'p str }

fn main() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let c = s1.len() > 1;
    let One { a } = if c { One { a: &s1 } } else { One { a: &s2 } };
    println!("{}", a);

    let t1 = String::from("a");
    let t2 = String::from("b");
    let (b, k) = if c { (&t1, 1) } else { (&t2, 2) };
    println!("{} {}", b, k);

    let u1 = String::from("c");
    let u2 = String::from("d");
    let n = u1.len();
    let [x] = match n {
        0 => [&u1],
        _ => [&u2],
    };
    println!("{}", x);

    let w1 = String::from("e");
    let w2 = String::from("f");
    let Some(y) = (if c { Some(&w1) } else { Some(&w2) }) else {
        return;
    };
    println!("{}", y);

    let h1 = String::from("g");
    let h2 = String::from("h");
    let (h, j) = if c { (Some(&h1), 1) } else { (Some(&h2), 2) };
    println!("{:?} {}", h, j);
}
