// #1370, #1338: a reference whose borrow ends in every arm of a
// conditional. Every arm that gets to the join runs on from where the
// borrow ended, dashed ("s's borrow has ended on this path"), to the join
// dot, the last arm too. main stopped the last arm's leg at its dot.
fn bump(r: &mut i32) {
    *r += 1;
}
fn bumpb(r: &mut i32) -> bool {
    *r += 1;
    *r > 2
}
fn look(x: &i32) -> bool {
    *x > 0
}
// The last arm uses the reference on its last row.
fn if_else(s: &mut String, c: bool) {
    if c {
        s.push('a');
    } else {
        s.push('b');
    }
}
fn by_match(s: &String, n: u8) {
    match n {
        0 => println!("{}", s),
        1 => println!("{}", s),
        _ => println!("{}", s),
    }
}
// The last arm doesn't use it: its borrow ends as it starts.
fn else_unused(s: &mut String, c: bool) {
    if c {
        s.push('a');
    } else {
    }
}
fn match_unused(s: &String, n: u8) {
    match n {
        0 => println!("{}", s),
        1 => {}
        _ => {}
    }
}
// The last arm's block uses it on an earlier row.
fn block_arm(s: &String, n: u8) {
    match n {
        0 => {}
        _ => {
            println!("{}", s);
            println!("{}", s);
        }
    }
}
// A local `&mut`.
fn local_mut(c: bool) {
    let mut t = String::new();
    let r = &mut t;
    if c {
        r.push('a');
    } else {
        r.push('b');
    }
    println!("{}", t);
}
// #1338: an `else if` whose condition is the reference's last use.
fn else_if_cond_mut(a: bool, b: bool) {
    let mut e = 0;
    let r = &mut e;
    if a {
    } else if b {
        bump(r);
    } else if bumpb(r) {
    } else {
    }
    println!("{}", e);
}
fn else_if_cond_shared(a: bool) {
    let e = 0;
    let r = &e;
    if a {
    } else if look(r) {
    } else {
    }
    println!("{}", e);
}
// `if let` whose `else` uses it.
fn if_let(o: Option<u8>, s: &mut String) {
    if let Some(_) = o {
        s.push('a');
    } else {
        s.push('b');
    }
}
fn main() {
    let mut t = String::from("t");
    if_else(&mut t, true);
    by_match(&t, 1);
    else_unused(&mut t, true);
    match_unused(&t, 1);
    block_arm(&t, 1);
    local_mut(true);
    else_if_cond_mut(false, false);
    else_if_cond_shared(false);
    if_let(None, &mut t);
}
