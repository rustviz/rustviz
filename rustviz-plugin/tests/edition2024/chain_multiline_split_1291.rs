// #1291: an `if let` / let-chain condition written over several lines
// (`if let Some(x) = o` / `&& bump(r)` / `{`). Its later parts run only
// on the way into the `if` arm, so the arms split on the `if` line, as
// for a condition on one line (#1279), and those parts are drawn in the
// `if` arm, with r's borrow ending on the dot of its last use there; the
// else arm starts with it ended. Main split on the `{` line and drew the
// parts in the arm on rows above where its column starts, the borrow end
// beside the arms. The same for a lone `if let` whose binding moves out
// of o (`lone_moved`, drawn above the split), and for a `match` guard on
// a line of its own (`guard_multi`: the borrow end stayed on r's column
// beside the arms).
fn bump(r: &mut i32) -> bool { *r += 1; *r > 2 }
fn take(_s: &mut String) {}
fn chain_multiline(o: Option<i32>) {
    let mut c = 0;
    let r4 = &mut c;
    if let Some(x) = o
        && bump(r4)
    {
        println!("{}", x);
    } else {
        println!("no");
    }
    println!("{}", c);
}
fn no_else(o: Option<i32>) {
    let mut c = 0;
    let r = &mut c;
    if let Some(x) = o
        && bump(r)
    {
        println!("{}", x);
    }
    println!("{}", c);
}
fn three_lines(o: Option<i32>) {
    let mut c = 0;
    let mut d = 0;
    let r = &mut c;
    let q = &mut d;
    if let Some(x) = o
        && bump(r)
        && bump(q)
    {
        println!("{}", x);
    } else {
        println!("no");
    }
    println!("{} {}", c, d);
}
fn else_if(o: Option<i32>, b: bool) {
    let mut c = 0;
    let r = &mut c;
    if b {
        println!("b");
    } else if let Some(x) = o
        && bump(r)
    {
        println!("{}", x);
    } else {
        println!("no");
    }
    println!("{}", c);
}
fn moved_binding(o: Option<String>, c: bool) {
    if let Some(x) = o
        && c
    {
        println!("{}", x);
    } else {
        println!("no");
    }
}
fn lone_moved(o: Option<String>) {
    if let Some(x) = o
    {
        println!("{}", x);
    } else {
        println!("no");
    }
}
fn chain_block(o: Option<i32>) {
    let mut v = String::from("v");
    let r = &mut v;
    if let Some(x) = o
        && { let wr = r; take(wr); true }
        && x > 5
    {
        println!("y");
    }
}
fn guard_multi(o: Option<i32>) {
    let mut c = 0;
    let r = &mut c;
    match o {
        Some(x)
            if bump(r) =>
        {
            println!("{}", x);
        }
        _ => {}
    }
    println!("{}", c);
}
fn main() {
    chain_multiline(Some(3));
    no_else(None);
    three_lines(Some(1));
    else_if(Some(2), false);
    moved_binding(None, true);
    lone_moved(None);
    chain_block(Some(6));
    guard_multi(Some(1));
}
