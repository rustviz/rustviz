// #1467: a loop whose body leaves it on every pass (`break`, `return`)
// never comes back round, but a `while` / `for` may not run its body at
// all, and a `break` or `?` before an assignment may skip it. A value a
// conditional above may have moved, then assigned in such a loop: the
// join said the old value is dropped "when it is assigned again", and
// with a let-chain the scope end said it is dropped unconditionally,
// though when the loop doesn't run it is dropped at the scope end, if
// the conditional didn't move it (a drop flag). A bare `loop` that
// reaches the assignment first always runs it (`c`, `k`). Each loop's
// note said it "may run many times"; it runs at most once.

fn for_loop(n: i32) {
    let mut a = Some(String::from("a"));
    if n > 1 {
        drop(a);
    }
    for _ in 0..n {
        a = Some(String::from("b"));
        break;
    }
}

fn while_after_let_chain() {
    let mut b = Some(String::from("a"));
    let go = b.is_some();
    if let Some(x) = b && x.len() > 1 {
    }
    while go {
        b = Some(String::from("b"));
        break;
    }
}

fn bare_loop(n: i32) {
    let mut c = Some(String::from("a"));
    if n > 1 {
        drop(c);
    }
    loop {
        c = Some(String::from("b"));
        break;
    }
}

fn while_let_return(v: Vec<i32>) {
    let mut d = Some(String::from("a"));
    if v.len() > 1 {
        drop(d);
    }
    let mut it = v.into_iter();
    while let Some(_) = it.next() {
        d = Some(String::from("b"));
        return;
    }
}

fn labeled(n: i32) {
    let mut e = Some(String::from("a"));
    if n > 1 {
        drop(e);
    }
    'outer: for _ in 0..n {
        for _ in 0..n {
            e = Some(String::from("b"));
            break 'outer;
        }
    }
}

fn break_before(n: i32) {
    let mut f = Some(String::from("a"));
    if n > 1 {
        drop(f);
    }
    for i in 0..n {
        if i > 5 {
            break;
        }
        f = Some(String::from("b"));
        break;
    }
}

fn bare_loop_break_before(c: bool) {
    let mut g = Some(String::from("a"));
    if c {
        drop(g);
    }
    loop {
        if c {
            break;
        }
        g = Some(String::from("b"));
        break;
    }
}

fn while_if_else(c: bool) {
    let mut h = Some(String::from("a"));
    if c {
        drop(h);
    }
    while c {
        if c {
            h = Some(String::from("b"));
        } else {
            h = Some(String::from("c"));
        }
        break;
    }
}

fn bare_loop_if_else(c: bool) {
    let mut k = Some(String::from("a"));
    if c {
        drop(k);
    }
    loop {
        if c {
            k = Some(String::from("b"));
        } else {
            k = Some(String::from("c"));
        }
        break;
    }
}

fn try_before(n: Option<i32>) -> Option<i32> {
    let mut w = Some(String::from("a"));
    if n.is_some() {
        drop(w);
    }
    loop {
        let _k = n?;
        w = Some(String::from("b"));
        break;
    }
    Some(1)
}

fn match_arm(n: i32) {
    let mut x = Some(String::from("a"));
    match n {
        1 => drop(x),
        _ => {}
    }
    for _ in 0..n {
        x = Some(String::from("b"));
        break;
    }
}

// A `let mut m;` local the loop assigns: there is no later iteration
// for the assignment to drop an earlier one's value.
fn let_local(n: i32) {
    let mut m;
    for _ in 0..n {
        m = String::from("b");
        println!("{}", m);
        break;
    }
}

// Moved before the loop, then assigned in it: after the loop it holds a
// value only if the loop ran.
fn moved_before(c: bool) {
    let mut u = String::from("a");
    drop(u);
    while c {
        u = String::from("b");
        break;
    }
    u = String::from("c");
    println!("{}", u);
}

// A struct field the loop assigns, and a whole struct.
struct P {
    name: String,
    n: i32,
}

fn field(c: bool) {
    let mut p = P { name: String::from("a"), n: 1 };
    if c {
        drop(p.name);
    }
    while c {
        p.name = String::from("b");
        break;
    }
    println!("{}", p.n);
}

fn whole(c: bool) {
    let mut q = P { name: String::from("a"), n: 1 };
    if c {
        drop(q);
    }
    while c {
        q = P { name: String::from("b"), n: 2 };
        break;
    }
}

fn main() {
    for_loop(2);
    while_after_let_chain();
    bare_loop(2);
    while_let_return(vec![1]);
    labeled(2);
    break_before(2);
    bare_loop_break_before(true);
    while_if_else(true);
    bare_loop_if_else(true);
    try_before(Some(2));
    match_arm(3);
    let_local(2);
    moved_before(true);
    field(true);
    whole(true);
}
