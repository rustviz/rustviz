// #1297: a value mutably lent in one branch and moved in another. The
// lending branch's dotted (lent) line reached the join, but the join dot
// was drawn a row lower, on the reference's last use, merged with "no
// longer mutably borrowed": the leg stopped in mid-air above it. The join
// dot is now where the legs end, on the conditional's `}` row; the loan's
// end keeps a dot of its own. In an `if`/`else`, a `match`, an `if`
// without `else` (the loan of u is dead on the path that moves it,
// #1178), and nested one level deeper.
fn issue(cond: bool) {
    let mut s = String::from("hello");
    let mut other = String::from("other");
    let mut r = &mut other;
    if cond {
        r = &mut s;
    } else {
        drop(s);
    }
    r.push_str("!");
}
fn in_match(n: i32) {
    let mut t = String::from("hello");
    let mut other = String::from("other");
    let mut r = &mut other;
    match n {
        0 => {
            r = &mut t;
        }
        1 => {
            drop(t);
        }
        _ => {}
    }
    r.push_str("!");
}
fn no_else(c: bool) {
    let mut u = String::from("a");
    let mut w = String::from("b");
    let mut q = &mut u;
    if c {
        q = &mut w;
        drop(u);
    }
    q.push('x');
}
fn nested(c: bool, d: bool) {
    let mut s = String::from("hello");
    let mut other = String::from("other");
    let mut r = &mut other;
    if c {
        if d {
            r = &mut s;
        } else {
            drop(s);
        }
    }
    r.push_str("!");
}
fn main() { issue(true); in_match(0); no_else(true); nested(true, true); }
